import { execFile } from "child_process";
import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath } from "url";
import { promisify } from "util";
import { describe, expect, it } from "vitest";

const execFileAsync = promisify(execFile);
const testDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(testDir, "../../../..");
const resultPrefix = "__POSTCSS_IMPLEMENTATION__";

// Isolate native workers and watchers so every scenario starts with a fresh
// processor and the child can release the entire development runtime on exit.
async function runFixture(scenario: string) {
  const projectPath = fs.mkdtempSync(
    path.join(os.tmpdir(), "utoo-postcss-implementation-"),
  );
  try {
    const { stdout } = await execFileAsync(
      process.execPath,
      [
        path.join(repoRoot, "node_modules/vite-node/vite-node.mjs"),
        path.join(testDir, "postcssImplementationChild.ts"),
        projectPath,
        scenario,
      ],
      {
        cwd: path.resolve(testDir, "../.."),
        timeout: 60_000,
        maxBuffer: 4 * 1024 * 1024,
        env: {
          ...process.env,
          NODE_PATH: [
            path.join(repoRoot, "node_modules"),
            process.env.NODE_PATH,
          ]
            .filter(Boolean)
            .join(path.delimiter),
        },
      },
    );
    const result = stdout
      .split(/\r?\n/)
      .find((line) => line.startsWith(resultPrefix));
    if (!result) throw new Error(`No fixture result: ${stdout}`);
    return JSON.parse(result.slice(resultPrefix.length));
  } finally {
    fs.rmSync(`${projectPath}.cjs`, { force: true });
    fs.rmSync(projectPath, { recursive: true, force: true });
  }
}

describe("PostCSS implementation", () => {
  it.each(["build", "server", "inline", "dev"])(
    "uses the configured processor and both plugin sources in %s",
    async (scenario) => {
      const result = await runFixture(scenario);
      expect(result.initial).toBe(true);
      if (scenario === "dev") expect(result.updated).toBe(true);
    },
    70_000,
  );

  it("runs discovered plugins with an implementation-only configuration", async () => {
    expect(await runFixture("config-only")).toEqual({ initial: true });
  }, 70_000);

  it("keeps default resolution when implementation is omitted", async () => {
    expect(await runFixture("default")).toEqual({ ambient: true });
  }, 70_000);

  it("does not fall back when the configured module is missing", async () => {
    expect(await runFixture("missing")).toEqual({ missing: true });
  }, 70_000);
});
