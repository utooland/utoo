import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { describe, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(testDir, "../../../..");
const execute = promisify(execFile);

describe("native bundle analyzer", () => {
  it.each([
    "single",
    "mpa",
    "html",
    "node",
    "mixed",
    "server-only",
    "server-functions",
    "server-functions-entry",
    "external-node",
    "external-mixed",
    "workers",
    "service-workers",
  ])(
    "exports %s entry sizes and import chains without changing build outputs or stats",
    async (scenario) => {
      const target = path.join(repoRoot, "target");
      fs.mkdirSync(target, { recursive: true });
      const project = fs.mkdtempSync(path.join(target, "native-analyzer-"));
      try {
        // Native projects own worker threads. A subprocess makes a failed native
        // task cancellable and also verifies that analysis shuts down cleanly.
        await execute(
          process.execPath,
          [
            path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
            path.join(testDir, "nativeAnalyzerChild.ts"),
            project,
            scenario,
          ],
          {
            cwd: path.resolve(testDir, "../.."),
            timeout: 80_000,
            killSignal: "SIGKILL",
            maxBuffer: 8 * 1024 * 1024,
            env: { ...process.env, UTOO_ROOT_PATH: project },
          },
        );
      } finally {
        fs.rmSync(project, { recursive: true, force: true });
      }
    },
    90_000,
  );
});
