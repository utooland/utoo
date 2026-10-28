import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const childScript = path.join(testDir, "libraryContentHashChild.ts");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");

function runLibraryContentHashFixture() {
  return new Promise<unknown>((resolve, reject) => {
    const targetDir = path.join(repoRoot, "target");
    fs.mkdirSync(targetDir, { recursive: true });
    const projectPath = fs.mkdtempSync(
      path.join(targetDir, "library-content-hash-"),
    );
    const child = spawn(
      process.execPath,
      [viteNode, childScript, projectPath],
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
    let settled = false;
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, 25_000);
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    const settle = (error: unknown | null, result?: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        fs.rmSync(projectPath, { recursive: true, force: true });
      } catch (cleanupError) {
        error ??= cleanupError;
      }
      if (error) reject(error);
      else resolve(result);
    };

    child.once("error", (error) => {
      settle(error);
    });
    child.once("close", (code, signal) => {
      if (settled) return;
      try {
        if (timedOut) {
          throw new Error("library content hash fixture timed out after 25s");
        }
        if (code !== 0) {
          throw new Error(
            `library content hash fixture failed with code ${code}, signal ${signal}\n${stderr}\n${stdout}`,
          );
        }
        const line = stdout
          .split(/\r?\n/)
          .find((value) => value.startsWith("__LIBRARY_CONTENT_HASH__"));
        if (!line) {
          throw new Error(
            `library content hash fixture did not print results\n${stderr}\n${stdout}`,
          );
        }
        settle(null, JSON.parse(line.slice("__LIBRARY_CONTENT_HASH__".length)));
      } catch (error) {
        settle(error);
      }
    });
  });
}

describe("library content hashes", () => {
  it("tracks the runtime library name and export path while identical builds remain stable", async () => {
    await expect(runLibraryContentHashFixture()).resolves.toEqual({
      sameConfigStableFilename: true,
      sameConfigStableBody: true,
      libraryNameChangesFilename: true,
      libraryNameChangesBody: true,
      exportPathChangesFilename: true,
      exportPathChangesBody: true,
      exports: [
        { type: "function", value: "default-widget" },
        { type: "function", value: "default-widget" },
        { type: "function", value: "default-widget" },
        { type: "function", value: "nested-widget" },
      ],
    });
  }, 30_000);
});
