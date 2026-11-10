import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { describe, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(testDir, "../../../..");
const execute = promisify(execFile);

describe("native module reference matching", () => {
  it.each([
    "enabled",
    "legacy",
    "legacy-tree-shaking",
    "modern-tree-shaking",
    "omitted",
    "empty",
    "symlink-file-resource",
    "symlink-directory-resource",
  ])(
    "preserves exact module boundaries with %s configuration",
    async (scenario) => {
      const target = path.join(repoRoot, "target");
      fs.mkdirSync(target, { recursive: true });
      const project = fs.mkdtempSync(path.join(target, "module-references-"));
      try {
        // Isolate native workers so a stalled compiler cannot hang the test runner.
        await execute(
          process.execPath,
          [
            path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
            path.join(testDir, "moduleReferencesBuildChild.ts"),
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
