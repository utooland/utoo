import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const resultPrefix = "__SWC_HELPERS__";

// Keep native worker threads and shutdown isolated from the Vitest process.
function runFixture(): Promise<unknown> {
  const targetDir = path.join(repoRoot, "target");
  fs.mkdirSync(targetDir, { recursive: true });
  const projectPath = fs.mkdtempSync(path.join(targetDir, "swc-helpers-"));

  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [
        path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
        path.join(testDir, "swcHelpersChild.ts"),
        projectPath,
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
    child.on("close", (code, signal) => {
      try {
        if (code !== 0) {
          throw new Error(
            `SWC helpers fixture failed (${code}, ${signal})\n${stderr}\n${stdout}`,
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

describe("SWC runtime helpers", () => {
  it("preserves named RegExp groups when compiling for older browsers", async () => {
    await expect(runFixture()).resolves.toEqual({
      word: "hello",
      matched: "hello-hello",
      rejectsDifferentWord: true,
      replacement: "[hello]",
    });
  }, 60_000);
});
