import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  analyze: vi.fn(async () => ({
    outputPath: "/fixture/.utoo/analyze",
    entries: [],
  })),
  serveAnalyzeReport: vi.fn(async () => ({ url: "http://127.0.0.1:4000" })),
  projectOptions: { config: { entry: [] } },
}));

vi.mock("@utoo/pack", () => mocks);
vi.mock("../../../pack-cli/src/utils/common", () => ({
  resolveBuildOptions: async () => ({
    projectOptions: mocks.projectOptions,
    projectPath: "/fixture",
    rootPath: "/fixture",
  }),
}));

import { run } from "../../../pack-cli/src/index";

let originalArgv: string[];
beforeEach(() => {
  vi.clearAllMocks();
  originalArgv = process.argv;
});
afterEach(() => {
  process.argv = originalArgv;
});

describe("native Analyzer CLI", () => {
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
