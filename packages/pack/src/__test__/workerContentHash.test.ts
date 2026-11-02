import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const resultPrefix = "__WORKER_CONTENT_HASH__";

type Scenario = "self" | "mutual" | "ordinary" | "self-sourcemap";

// A stuck native task cannot be cancelled by Vitest. Keep each graph and its
// persistent-cache rebuilds in a child process so this regression fails cleanly.
function runFixture(scenario: Scenario): Promise<{ scenario: Scenario }> {
  const targetDir = path.join(repoRoot, "target");
  fs.mkdirSync(targetDir, { recursive: true });
  const projectPath = fs.mkdtempSync(
    path.join(targetDir, "worker-content-hash-"),
  );

  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [
        path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
        path.join(testDir, "workerContentHashChild.ts"),
        projectPath,
        scenario,
      ],
      {
        cwd: packageRoot,
        env: {
          ...process.env,
          // Fixtures have isolated caches; enable persistence even while the
          // surrounding source checkout contains the fix under test.
          TURBO_ENGINE_IGNORE_DIRTY: "1",
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
            `worker content hash ${scenario} failed (${code}, ${signal})\n${stderr}\n${stdout}`,
          );
        }
        const line = stdout
          .split(/\r?\n/)
          .find((item) => item.startsWith(resultPrefix));
        if (!line) {
          throw new Error(`Missing fixture result\n${stderr}\n${stdout}`);
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

describe("worker content hashes", () => {
  it.each(["self", "mutual", "ordinary", "self-sourcemap"] as const)(
    "builds %s workers and invalidates cached worker URLs after content changes",
    async (scenario) => {
      await expect(runFixture(scenario)).resolves.toEqual({ scenario });
    },
    60_000,
  );
});
