import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const execute = promisify(execFile);

it("resolves a linked package and its SSR alias through a configured additional root", async () => {
  const target = path.join(repoRoot, "target");
  fs.mkdirSync(target, { recursive: true });
  const fixture = fs.mkdtempSync(path.join(target, "additional-roots-"));
  try {
    await execute(
      process.execPath,
      [
        path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
        path.join(testDir, "additionalRootsChild.ts"),
        fixture,
      ],
      {
        cwd: packageRoot,
        timeout: 80_000,
        killSignal: "SIGKILL",
        env: {
          ...process.env,
          UTOO_ROOT_PATH: "",
          TURBO_ENGINE_IGNORE_DIRTY: "1",
          TURBO_ENGINE_SNAPSHOT_MIN_ACTIVE_TIME_MILLIS: "0",
        },
      },
    );
  } finally {
    fs.rmSync(fixture, { recursive: true, force: true });
  }
}, 90_000);
