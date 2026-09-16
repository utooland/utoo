import path from "path";
import { describe, expect, it } from "vitest";
import { resolveCacheDirectory } from "./cacheDirectory";

const projectPath = path.resolve("/workspace/app");

describe("resolveCacheDirectory", () => {
  it("defaults to .turbopack inside the project path", () => {
    expect(resolveCacheDirectory(projectPath, undefined)).toBe(
      path.join(projectPath, ".turbopack"),
    );
    expect(resolveCacheDirectory(projectPath, "")).toBe(
      path.join(projectPath, ".turbopack"),
    );
  });

  it("resolves relative directories from the project path", () => {
    expect(
      resolveCacheDirectory(projectPath, "node_modules/.cache/utoopack"),
    ).toBe(path.join(projectPath, "node_modules", ".cache", "utoopack"));
    expect(resolveCacheDirectory(projectPath, "../shared-cache")).toBe(
      path.resolve(projectPath, "..", "shared-cache"),
    );
  });

  it("keeps absolute directories as-is", () => {
    const absolute = path.resolve("/tmp/utoopack-cache");

    expect(resolveCacheDirectory(projectPath, absolute)).toBe(absolute);
  });
});
