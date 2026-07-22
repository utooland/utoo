import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");

function runServeStatsFixture(childScriptName = "serveStatsChild.ts") {
  return new Promise<unknown>((resolve, reject) => {
    const projectPath = fs.mkdtempSync(
      path.join(repoRoot, "target/serve-stats-"),
    );
    const port = 43_200 + Math.floor(Math.random() * 1000);
    const childScript = path.join(testDir, childScriptName);
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
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code !== 0) {
        reject(
          new Error(
            `serve stats fixture failed with code ${code}, signal ${signal}\n${stderr}\n${stdout}`,
          ),
        );
        return;
      }

      const line = stdout
        .split(/\r?\n/)
        .find((item) => item.startsWith("__STATS_SNAPSHOT__"));
      if (!line) {
        reject(
          new Error(
            `serve stats fixture did not print stats\n${stderr}\n${stdout}`,
          ),
        );
        return;
      }

      resolve(JSON.parse(line.slice("__STATS_SNAPSHOT__".length)));
    });
  });
}

describe("serve stats", () => {
  it("writes webpack stats.json in dev mode when stats are enabled", async () => {
    await expect(runServeStatsFixture()).resolves.toMatchInlineSnapshot(`
      {
        "assets": [
          {
            "name": "_root-of-the-server___<hash>.js",
            "type": "asset",
          },
          {
            "name": "_root-of-the-server___<hash>.js.map",
            "type": "asset",
          },
          {
            "name": "main.js",
            "type": "asset",
          },
          {
            "name": "main.js.map",
            "type": "asset",
          },
          {
            "name": "src_index_<hash>.js",
            "type": "asset",
          },
          {
            "name": "src_lazy_<hash>.js",
            "type": "asset",
          },
          {
            "name": "src_lazy_<hash>.js",
            "type": "asset",
          },
          {
            "name": "src_lazy_<hash>.js.map",
            "type": "asset",
          },
          {
            "name": "src_lazy_<hash>.js.map",
            "type": "asset",
          },
        ],
        "entrypoints": {
          "main": {
            "assets": [
              "_root-of-the-server___<hash>.js",
              "main.js",
              "src_index_<hash>.js",
              "src_lazy_<hash>.js",
            ],
            "chunks": [
              "_root-of-the-server___<hash>.js",
              "main.js",
              "src_index_<hash>.js",
              "src_lazy_<hash>.js",
            ],
          },
        },
        "htmlGenerated": true,
      }
    `);
  }, 30_000);

  it("keeps all named server entries after rebuilding one entry", async () => {
    await expect(
      runServeStatsFixture("serveMultiServerStatsChild.ts"),
    ).resolves.toEqual({
      changedEntry: true,
      initialEntries: ["detail-server", "index-server", "server"],
      preservedSharedAsset: true,
      preservedEntries: true,
      rebuiltEntries: ["detail-server", "index-server", "server"],
      sharedChangeInvalidatedEntries: true,
    });
  }, 30_000);
});
