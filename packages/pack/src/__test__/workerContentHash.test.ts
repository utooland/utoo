import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { describe, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(testDir, "../../../..");
const execute = promisify(execFile);

describe("worker content hashes", () => {
  it.each(["self", "mutual", "ordinary", "self-sourcemap"])(
    "preserves cached hashes and SharedWorker identity for %s",
    async (scenario) => {
      const target = path.join(repoRoot, "target");
      fs.mkdirSync(target, { recursive: true });
      const project = fs.mkdtempSync(path.join(target, "worker-content-hash-"));
      try {
        // Native task cycles cannot be cancelled inside Vitest.
        await execute(
          process.execPath,
          [
            path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
            path.join(testDir, "workerContentHashChild.ts"),
            project,
            scenario,
          ],
          {
            timeout: 50_000,
            killSignal: "SIGKILL",
            maxBuffer: 8 * 1024 * 1024,
            env: {
              ...process.env,
              TURBO_ENGINE_IGNORE_DIRTY: "1",
              TURBO_ENGINE_SNAPSHOT_MIN_ACTIVE_TIME_MILLIS: "0",
            },
          },
        );
      } finally {
        fs.rmSync(project, { recursive: true, force: true });
      }
    },
    60_000,
  );
});
