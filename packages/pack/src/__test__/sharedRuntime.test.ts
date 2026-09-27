import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const resultPrefix = "__SHARED_RUNTIME__";

type Scenario = "production" | "development" | "node";

// Native projects own worker threads. Isolate each scenario, including shutdown,
// so failures cannot leave native state behind in the Vitest process.
function runFixture(scenario: Scenario): Promise<{ scenario: Scenario }> {
  const targetDir = path.join(repoRoot, "target");
  fs.mkdirSync(targetDir, { recursive: true });
  const projectPath = fs.mkdtempSync(path.join(targetDir, "shared-runtime-"));

  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [
        path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
        path.join(testDir, "sharedRuntimeChild.ts"),
        projectPath,
        scenario,
      ],
      {
        cwd: packageRoot,
        env: {
          ...process.env,
          NODE_PATH: [
            path.join(packageRoot, "node_modules"),
            path.join(repoRoot, "node_modules"),
            process.env.NODE_PATH,
          ]
            .filter(Boolean)
            .join(path.delimiter),
        },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    const timeout = setTimeout(() => child.kill("SIGKILL"), 50_000);
    const cleanup = () => {
      clearTimeout(timeout);
      fs.rmSync(projectPath, { recursive: true, force: true });
    };
    child.on("error", (error) => {
      cleanup();
      reject(error);
    });
    child.on("exit", (code, signal) => {
      try {
        if (code !== 0) {
          throw new Error(
            `shared runtime ${scenario} failed (${code}, ${signal})\n${stderr}\n${stdout}`,
          );
        }
        const line = stdout
          .split(/\r?\n/)
          .find((item) => item.startsWith(resultPrefix));
        if (!line)
          throw new Error(`Missing fixture result\n${stderr}\n${stdout}`);
        resolve(JSON.parse(line.slice(resultPrefix.length)));
      } catch (error) {
        reject(error);
      } finally {
        cleanup();
      }
    });
  });
}

describe("shared browser runtime", () => {
  it("includes the shared runtime after each bootstrap in generated HTML", async () => {
    await expect(runFixture("production")).resolves.toEqual({
      scenario: "production",
    });
  }, 60_000);

  it.each(["development", "node"] as const)(
    "does not change %s output",
    async (scenario) => {
      await expect(runFixture(scenario)).resolves.toEqual({ scenario });
    },
    60_000,
  );
});
