import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BundleOptions, ConfigComplete } from "../config/types";

const mocks = vi.hoisted(() => ({
  createProject: vi.fn(),
  writeAnalyzeData: vi.fn(),
  shutdown: vi.fn(),
  getPackPath: vi.fn(),
  acquireLock: vi.fn(),
  unlock: vi.fn(),
}));

vi.mock("../core/project", () => ({
  projectFactory: () => mocks.createProject,
}));
vi.mock("../utils/common", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../utils/common")>()),
  getPackPath: mocks.getPackPath,
  blockStdout: vi.fn(),
}));
vi.mock("../utils/lockfile", () => ({
  acquirePersistentCacheLock: mocks.acquireLock,
}));
// HTML conversion has its own tests; this suite exercises lifecycle and files
// without loading either native addon in the Vitest process.
vi.mock("../utils/htmlEntry", () => ({ processHtmlEntry: vi.fn() }));

import { analyze } from "../commands/analyze";

let fixture: string;
let projectPath: string;
let reportPath: string;
let options: BundleOptions;
const entries = [{ name: "main", path: "/entry-0" }];

function oldReport() {
  fs.mkdirSync(reportPath, { recursive: true });
  fs.writeFileSync(
    path.join(reportPath, "utoo-analyzer.json"),
    JSON.stringify({ tool: "@utoo/pack-analyzer", version: 1 }),
  );
  fs.writeFileSync(path.join(reportPath, "old.txt"), "previous report");
}

function assertOldReportPreserved() {
  expect(fs.readFileSync(path.join(reportPath, "old.txt"), "utf8")).toBe(
    "previous report",
  );
  expect(fs.readdirSync(path.dirname(reportPath)).sort()).toEqual(["report"]);
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubEnv("UTOO_ROOT_PATH", undefined);
  fixture = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-analyze-lifecycle-"));
  projectPath = path.join(fixture, "project");
  reportPath = path.join(fixture, "reports/report");
  fs.mkdirSync(projectPath, { recursive: true });
  fs.writeFileSync(path.join(projectPath, "entry.js"), "console.log('entry');");
  const packPath = path.join(fixture, "pack/src");
  const uiPath = path.join(fixture, "pack/analyzer");
  fs.mkdirSync(uiPath, { recursive: true });
  fs.writeFileSync(path.join(uiPath, "index.html"), "<html>Analyzer</html>");
  mocks.getPackPath.mockReturnValue(packPath);
  mocks.shutdown.mockResolvedValue(undefined);
  mocks.acquireLock.mockResolvedValue({ unlockSync: mocks.unlock });
  mocks.createProject.mockResolvedValue({
    writeAnalyzeData: mocks.writeAnalyzeData,
    shutdown: mocks.shutdown,
  });
  mocks.writeAnalyzeData.mockImplementation(async (outputPath: string) => {
    fs.mkdirSync(outputPath, { recursive: true });
    fs.writeFileSync(path.join(outputPath, "modules.data"), "native report");
    return { entries, issues: [] };
  });
  options = {
    config: {
      entry: [{ name: "main", import: "./entry.js", html: {} }],
      mode: "development",
      sourceMaps: false,
      stats: true,
      output: { path: "dist", clean: true },
    },
    dev: true,
    tracing: false,
  };
});

afterEach(() => {
  vi.unstubAllEnvs();
  fs.rmSync(fixture, { recursive: true, force: true });
});

function run() {
  return analyze(options, projectPath, projectPath, { outputPath: reportPath });
}

describe("native analysis lifecycle", () => {
  it("uses a private production configuration and preserves existing bundles and stats", async () => {
    const callerOptions = JSON.stringify(options);
    const distPath = path.join(projectPath, "dist");
    fs.mkdirSync(distPath, { recursive: true });
    fs.writeFileSync(path.join(distPath, "stats.json"), "old stats");
    fs.writeFileSync(path.join(distPath, "app.js"), "old bundle");
    vi.stubEnv("ANALYZE", "true");

    await expect(run()).resolves.toEqual({ outputPath: reportPath, entries });

    expect(mocks.createProject.mock.calls[0][0]).toMatchObject({
      config: { mode: "production", sourceMaps: true, stats: false },
      watch: { enable: false },
      dev: false,
    });
    expect(JSON.stringify(options)).toBe(callerOptions);
    expect(fs.readdirSync(distPath).sort()).toEqual(["app.js", "stats.json"]);
    expect(fs.readFileSync(path.join(distPath, "stats.json"), "utf8")).toBe(
      "old stats",
    );
    expect(fs.readFileSync(path.join(distPath, "app.js"), "utf8")).toBe(
      "old bundle",
    );
    expect(
      JSON.parse(
        fs.readFileSync(path.join(reportPath, "data/entries.json"), "utf8"),
      ),
    ).toEqual(entries);
    expect(mocks.shutdown).toHaveBeenCalledOnce();
    expect(mocks.unlock).toHaveBeenCalledOnce();
  });

  it("replaces an earlier completed report after successful analysis", async () => {
    oldReport();
    mocks.shutdown.mockImplementation(async () => {
      // Publication happens after native resources have been released.
      expect(fs.readFileSync(path.join(reportPath, "old.txt"), "utf8")).toBe(
        "previous report",
      );
    });
    await run();
    expect(fs.existsSync(path.join(reportPath, "old.txt"))).toBe(false);
    expect(
      fs.readFileSync(path.join(reportPath, "data/modules.data"), "utf8"),
    ).toBe("native report");
    expect(fs.readdirSync(path.dirname(reportPath))).toEqual(["report"]);
  });

  it.each(["native error", "compiler issue"])(
    "preserves the old report, cleans staging and releases resources on %s",
    async (failure) => {
      oldReport();
      mocks.writeAnalyzeData.mockImplementation(async (outputPath: string) => {
        fs.mkdirSync(outputPath, { recursive: true });
        fs.writeFileSync(
          path.join(outputPath, "partial.data"),
          "partial result",
        );
        if (failure === "native error") throw new Error("analysis failed");
        return {
          entries,
          issues: [
            {
              severity: "error",
              stage: "parse",
              filePath: "[project]/entry.js",
              title: { type: "text", value: "analysis failed" },
              documentationLink: "",
              importTraces: [],
            },
          ],
        };
      });
      await expect(run()).rejects.toThrow("analysis failed");
      assertOldReportPreserved();
      expect(mocks.shutdown).toHaveBeenCalledOnce();
      expect(mocks.unlock).toHaveBeenCalledOnce();
    },
  );

  it("does not overwrite an arbitrary output directory", async () => {
    fs.mkdirSync(reportPath, { recursive: true });
    fs.writeFileSync(path.join(reportPath, "keep.txt"), "user data");
    await expect(run()).rejects.toThrow("not a Pack Analyzer report");
    expect(fs.readFileSync(path.join(reportPath, "keep.txt"), "utf8")).toBe(
      "user data",
    );
    expect(mocks.createProject).not.toHaveBeenCalled();
  });

  it("accepts a browser framework configuration with an empty server entry list", async () => {
    options.config.server = { entry: [] };
    await expect(run()).resolves.toEqual({ outputPath: reportPath, entries });
    expect(mocks.createProject).toHaveBeenCalledOnce();
  });

  it.each([
    { target: "current node" },
    { target: "last 1 node version" },
    { entry: [{ name: "lib", import: "./entry.js", library: {} }] },
  ] satisfies Partial<ConfigComplete>[])(
    "rejects unsupported builds before creating a native project: %j",
    async (unsupported) => {
      options.config = { ...options.config, ...unsupported };
      await expect(run()).rejects.toThrow(
        "browser application entries and MPA",
      );
      expect(mocks.createProject).not.toHaveBeenCalled();
      expect(fs.existsSync(path.dirname(reportPath))).toBe(false);
    },
  );
});
