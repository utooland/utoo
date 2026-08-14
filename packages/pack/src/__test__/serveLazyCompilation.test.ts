import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const childScript = path.join(testDir, "serveLazyCompilationChild.ts");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");

function runLazyCompilationFixture() {
  return new Promise<unknown>((resolve, reject) => {
    const targetDir = path.join(repoRoot, "target");
    fs.mkdirSync(targetDir, { recursive: true });
    const projectPath = fs.mkdtempSync(
      path.join(targetDir, "serve-lazy-compilation-"),
    );
    const port = 45_200 + Math.floor(Math.random() * 1000);
    const child = spawn(
      process.execPath,
      [viteNode, childScript, projectPath, `${port}`],
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
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
    }, 60_000);

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
      clearTimeout(timeout);
      cleanup();
      reject(error);
    });
    child.on("exit", (code, signal) => {
      clearTimeout(timeout);
      try {
        if (code !== 0) {
          throw new Error(
            `serve lazy compilation fixture failed with code ${code}, signal ${signal}\n${stderr}\n${stdout}`,
          );
        }

        const line = stdout
          .split(/\r?\n/)
          .find((item) => item.startsWith("__LAZY_COMPILATION_RESULT__"));
        if (!line) {
          throw new Error(
            `serve lazy compilation fixture did not print a result\n${stderr}\n${stdout}`,
          );
        }

        resolve(JSON.parse(line.slice("__LAZY_COMPILATION_RESULT__".length)));
      } catch (error) {
        reject(error);
      } finally {
        cleanup();
      }
    });
  });
}

describe("serve lazy compilation", () => {
  it("serves copied and dynamic assets on demand with partial HMR", async () => {
    await expect(runLazyCompilationFixture()).resolves.toEqual({
      copiedAssetMaterializedAtReady: false,
      copiedAssetResponseContainsMarker: true,
      copiedAssetResponseStatus: 200,
      copiedAssetUpdateObserved: true,
      entryResponsesSucceeded: true,
      entryResponseContainsLazyMarker: false,
      expandedRoutesSurvivedEviction: true,
      hmrChunkListPathDiscovered: true,
      headResponseContentLength: true,
      headResponseLength: 0,
      headResponseStatus: 200,
      hmrPartialUpdateReceived: true,
      hmrReloadReceived: false,
      lazyLoaderInvokedAtReady: false,
      lazyMaterializedAtReady: false,
      lazyResponseContainsMarker: true,
      lazyResponseStatus: 200,
      rangedResponseContentRange: true,
      rangedResponseLength: 10,
      rangedResponseStatus: 206,
      sourceMapResponseIsJson: true,
      sourceMapResponseStatus: 200,
    });
  }, 65_000);
});
