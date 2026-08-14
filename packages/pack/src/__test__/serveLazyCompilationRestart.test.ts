import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { afterAll, describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const childScript = path.join(testDir, "serveLazyCompilationRestartChild.ts");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");
const targetDir = path.join(repoRoot, "target");
fs.mkdirSync(targetDir, { recursive: true });
const projectPath = fs.mkdtempSync(
  path.join(targetDir, "serve-lazy-compilation-restart-"),
);

afterAll(() => {
  fs.rmSync(projectPath, { recursive: true, force: true });
});

function runFixture(initialize: boolean) {
  return new Promise<unknown>((resolve, reject) => {
    const port = 46_200 + Math.floor(Math.random() * 1000);
    const child = spawn(
      process.execPath,
      [viteNode, childScript, projectPath, `${port}`, `${initialize}`],
      {
        cwd: packageRoot,
        env: {
          ...process.env,
          TURBO_ENGINE_EVICT_AFTER_SNAPSHOT: "1",
          TURBO_ENGINE_IGNORE_DIRTY: "1",
          TURBO_ENGINE_SNAPSHOT_IDLE_TIMEOUT_MILLIS: "0",
          TURBO_ENGINE_SNAPSHOT_MIN_ACTIVE_TIME_MILLIS: "0",
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
    const timeout = setTimeout(() => child.kill("SIGKILL"), 30_000);
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      clearTimeout(timeout);
      try {
        if (code !== 0) {
          throw new Error(
            `lazy restart fixture failed with code ${code}, signal ${signal}\n${stderr}\n${stdout}`,
          );
        }
        const line = stdout
          .split(/\r?\n/)
          .find((item) => item.startsWith("__LAZY_RESTART_RESULT__"));
        if (!line) {
          throw new Error(
            `lazy restart fixture printed no result\n${stderr}\n${stdout}`,
          );
        }
        resolve(JSON.parse(line.slice("__LAZY_RESTART_RESULT__".length)));
      } catch (error) {
        reject(error);
      }
    });
  });
}

describe("serve lazy compilation across restarts", () => {
  it("re-primes entry assets in a fresh process", async () => {
    await expect(runFixture(true)).resolves.toEqual({
      clientPathCount: 2,
      statuses: [200, 200],
    });
    await expect(runFixture(false)).resolves.toEqual({
      clientPathCount: 2,
      statuses: [200, 200],
    });
  }, 65_000);
});
