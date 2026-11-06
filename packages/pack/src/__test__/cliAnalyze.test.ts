import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  analyze: vi.fn(async () => ({
    outputPath: "/fixture/.utoo/analyze",
    entries: [],
  })),
  serveAnalyzeReport: vi.fn(async () => ({ url: "http://127.0.0.1:4000" })),
  projectOptions: { config: { entry: [] } },
  exportAnalyzeReport: vi.fn(async function* () {
    yield { type: "meta", schema_version: 1 };
    yield { type: "entry", name: "home", environment: "server" };
  }),
  resolveBuildOptions: vi.fn(),
  writes: [] as string[],
}));

vi.mock("@utoo/pack", () => mocks);
vi.mock("@utoo/pack/analyze", () => ({
  exportAnalyzeReport: mocks.exportAnalyzeReport,
}));
vi.mock("node:fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:fs")>()),
  writeSync: (_fd: number, buffer: Buffer, offset: number, length: number) => {
    const written = Math.min(length, 7);
    mocks.writes.push(buffer.subarray(offset, offset + written).toString());
    return written;
  },
}));
vi.mock("../../../pack-cli/src/utils/common", () => ({
  resolveBuildOptions: mocks.resolveBuildOptions,
}));

import { run } from "../../../pack-cli/src/index";

let originalArgv: string[];
beforeEach(() => {
  vi.clearAllMocks();
  mocks.writes.length = 0;
  mocks.resolveBuildOptions.mockResolvedValue({
    projectOptions: mocks.projectOptions,
    projectPath: "/fixture",
    rootPath: "/fixture",
  });
  originalArgv = process.argv;
});
afterEach(() => {
  process.argv = originalArgv;
});

describe("native Analyzer CLI", () => {
  it("streams saved JSONL without loading project config or compiling", async () => {
    await run([
      "analyze",
      "export",
      "--report",
      "/fixture/saved report",
      "--entry",
      "home",
      "--environment",
      "server",
    ]);
    expect(mocks.exportAnalyzeReport).toHaveBeenCalledExactlyOnceWith(
      "/fixture/saved report",
      { entry: "home", environment: "server" },
    );
    expect(mocks.resolveBuildOptions).not.toHaveBeenCalled();
    expect(mocks.analyze).not.toHaveBeenCalled();
    expect(mocks.serveAnalyzeReport).not.toHaveBeenCalled();
    expect(mocks.writes.join("")).toBe(
      '{"type":"meta","schema_version":1}\n{"type":"entry","name":"home","environment":"server"}\n',
    );
  });
  it("exports a report without starting a server", async () => {
    process.argv = [
      "node",
      "up",
      "analyze",
      "--output",
      "--output-dir",
      "reports/bundle",
    ];
    await run();
    expect(mocks.analyze).toHaveBeenCalledExactlyOnceWith(
      mocks.projectOptions,
      "/fixture",
      "/fixture",
      { outputPath: "reports/bundle" },
    );
    expect(mocks.serveAnalyzeReport).not.toHaveBeenCalled();
  });
  it("serves the completed report using the requested port", async () => {
    process.argv = ["node", "up", "analyze", "--port", "4100"];
    await run();
    expect(mocks.analyze).toHaveBeenCalledOnce();
    expect(mocks.serveAnalyzeReport).toHaveBeenCalledExactlyOnceWith(
      "/fixture/.utoo/analyze",
      4100,
    );
    expect(mocks.analyze.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.serveAnalyzeReport.mock.invocationCallOrder[0],
    );
  });
});
