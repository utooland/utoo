import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const childScript = path.join(testDir, "serveClientPathsChild.ts");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");

function runServeClientPathsFixture() {
  return new Promise<unknown>((resolve, reject) => {
    const targetDir = path.join(repoRoot, "target");
    fs.mkdirSync(targetDir, { recursive: true });
    const projectPath = fs.mkdtempSync(
      path.join(targetDir, "serve-client-paths-"),
    );
    const port = 44_200 + Math.floor(Math.random() * 1000);
    const child = spawn(
      process.execPath,
      [viteNode, childScript, projectPath, `${port}`],
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
    const cleanup = () => {
      fs.rmSync(projectPath, { recursive: true, force: true });
    };

    child.on("error", (error) => {
      cleanup();
      reject(error);
    });
    child.on("exit", (code, signal) => {
      try {
        if (code !== 0) {
          throw new Error(
            `serve client paths fixture failed with code ${code}, signal ${signal}\n${stderr}\n${stdout}`,
          );
        }

        const line = stdout
          .split(/\r?\n/)
          .find((item) => item.startsWith("__CLIENT_PATHS_SNAPSHOT__"));
        if (!line) {
          throw new Error(
            `serve client paths fixture did not print client paths\n${stderr}\n${stdout}`,
          );
        }

        resolve(JSON.parse(line.slice("__CLIENT_PATHS_SNAPSHOT__".length)));
      } catch (error) {
        reject(error);
      } finally {
        cleanup();
      }
    });
  });
}

describe("serve client paths", () => {
  it("exposes initial client paths without webpack stats", async () => {
    await expect(runServeClientPathsFixture()).resolves.toMatchInlineSnapshot(`
      {
        "clientPaths": [
          "main.js",
          "src_index_<hash>.js",
        ],
        "statsGenerated": false,
      }
    `);
  }, 30_000);
});
