import { spawn } from "child_process";
import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(testDir, "../..");
const repoRoot = path.resolve(packageRoot, "../..");
const viteNode = path.join(repoRoot, "node_modules/vite-node/vite-node.mjs");

async function runSignalsFixture(
  secondSignal: "SIGTERM" | "SIGINT",
  failClose = false,
  firstSignal: "SIGTERM" | "SIGINT" = "SIGTERM",
) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-signals-"));
  try {
    fs.mkdirSync(path.join(directory, "dist"));
    return await new Promise<{
      code: number | null;
      signal: NodeJS.Signals | null;
      flushed: { closeCalls: number } | null;
      output: string;
    }>((resolve, reject) => {
      const child = spawn(
        process.execPath,
        [
          viteNode,
          "--config",
          path.join(testDir, "serveSignals.fixture.config.ts"),
          path.join(testDir, "serveSignalsChild.ts"),
        ],
        {
          cwd: packageRoot,
          env: {
            ...process.env,
            UTOO_SIGNAL_TEST_DIR: directory,
            UTOO_SIGNAL_TEST_FAIL: failClose ? "1" : "0",
          },
          stdio: ["ignore", "pipe", "pipe", "ipc"],
        },
      );
      let output = "";
      let release: ReturnType<typeof setTimeout> | undefined;
      let timedOut = false;
      const timeout = setTimeout(() => {
        timedOut = true;
        child.kill("SIGKILL");
      }, 15_000);
      child.stdout.on("data", (chunk) => {
        output += chunk;
      });
      child.stderr.on("data", (chunk) => {
        output += chunk;
      });
      child.on("message", (message) => {
        if (message === "ready") child.kill(firstSignal);
        if (message === "closing") {
          child.kill(secondSignal);
          // Keep the flush pending long enough for the second OS signal to run.
          release = setTimeout(() => {
            if (child.connected) child.send("release");
          }, 100);
        }
      });
      child.once("error", (error) => {
        child.kill("SIGKILL");
        clearTimeout(timeout);
        clearTimeout(release);
        reject(error);
      });
      child.once("close", (code, signal) => {
        clearTimeout(timeout);
        clearTimeout(release);
        if (timedOut) {
          reject(new Error(`Signal fixture timed out\n${output}`));
          return;
        }
        const marker = path.join(directory, "flushed.json");
        resolve({
          code,
          signal,
          flushed: fs.existsSync(marker)
            ? JSON.parse(fs.readFileSync(marker, "utf8"))
            : null,
          output,
        });
      });
    });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

describe.skipIf(process.platform === "win32")("dev server signals", () => {
  it("finishes shutdown after repeated SIGTERM", async () => {
    const result = await runSignalsFixture("SIGTERM");

    expect(result.signal, result.output).toBeNull();
    expect(result.code, result.output).toBe(0);
    expect(result.flushed).toEqual({ closeCalls: 1 });
  }, 20_000);

  it("preserves forced exit on a second SIGINT", async () => {
    const result = await runSignalsFixture("SIGINT", false, "SIGINT");

    expect(result.signal, result.output).toBe("SIGINT");
    expect(result.code, result.output).toBeNull();
    expect(result.flushed).toBeNull();
  }, 20_000);

  it("preserves a failed shutdown exit after repeated SIGTERM", async () => {
    const result = await runSignalsFixture("SIGTERM", true);

    expect(result.signal, result.output).toBeNull();
    expect(result.code, result.output).toBe(1);
    expect(result.flushed).toEqual({ closeCalls: 1 });
    expect(result.output).toContain("Cache persistence failed");
  }, 20_000);
});
