import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

type Scenario = "dist-root" | "server-dist-root";

type FixtureResult = {
  initial: string;
  scenario: Scenario;
  updated: string;
};

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const childScript = path.join(testDir, "serveServerOutputsHmrChild.ts");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");
const resultPrefix = "__SERVER_OUTPUT_HMR__";

/*
 * `serve()` owns native workers, file watchers, and a long-lived HTTP server.
 * Run each fixture in a child process so the test can terminate that complete
 * runtime cleanly after observing the rebuilt output.
 */
function runServerOutputHmrFixture(scenario: Scenario): Promise<FixtureResult> {
  return new Promise((resolve, reject) => {
    const targetDir = path.join(repoRoot, "target");
    fs.mkdirSync(targetDir, { recursive: true });
    const projectPath = fs.mkdtempSync(
      path.join(targetDir, `serve-server-output-${scenario}-`),
    );
    const port = 45_200 + Math.floor(Math.random() * 1000);
    const child = spawn(
      process.execPath,
      [viteNode, childScript, projectPath, `${port}`, scenario],
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
    let timedOut = false;

    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    const cleanup = () => {
      fs.rmSync(projectPath, { recursive: true, force: true });
    };
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
      cleanup();
      reject(
        new Error(
          `serve server output fixture timed out for ${scenario}\n${stderr}\n${stdout}`,
        ),
      );
    }, 50_000);

    child.on("error", (error) => {
      clearTimeout(timeout);
      cleanup();
      reject(error);
    });
    child.on("exit", (code, signal) => {
      clearTimeout(timeout);
      if (timedOut) {
        return;
      }

      try {
        if (code !== 0) {
          throw new Error(
            `serve server output fixture failed with code ${code}, signal ${signal}\n${stderr}\n${stdout}`,
          );
        }

        const line = stdout
          .split(/\r?\n/)
          .find((item) => item.startsWith(resultPrefix));
        if (!line) {
          throw new Error(
            `serve server output fixture did not print a result\n${stderr}\n${stdout}`,
          );
        }

        resolve(JSON.parse(line.slice(resultPrefix.length)));
      } catch (error) {
        reject(error);
      } finally {
        cleanup();
      }
    });
  });
}

describe("serve server output HMR", () => {
  it.each(["dist-root", "server-dist-root"] as const)(
    "rewrites changed server assets under %s",
    async (scenario) => {
      await expect(runServerOutputHmrFixture(scenario)).resolves.toEqual({
        initial: "SERVER_OUTPUT_V1",
        scenario,
        updated: "SERVER_OUTPUT_V2",
      });
    },
    60_000,
  );
});
