import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { findRootDir } from "../utils/findRoot";
import { resolveRootPath } from "../utils/resolveRootPath";

let fixture: string;
let projectPath: string;

beforeEach(() => {
  vi.stubEnv("UTOO_ROOT_PATH", undefined);
  fixture = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-root-path-"));
  projectPath = path.join(fixture, "worktree");
  fs.mkdirSync(projectPath);
  fs.writeFileSync(path.join(projectPath, "package-lock.json"), "{}");
});

afterEach(() => {
  vi.unstubAllEnvs();
  fs.rmSync(fixture, { recursive: true, force: true });
});

describe("resolveRootPath", () => {
  it("overrides the configured root with a common worktree parent", () => {
    vi.stubEnv("UTOO_ROOT_PATH", fixture);

    expect(resolveRootPath(projectPath, projectPath)).toBe(fixture);
  });

  it("resolves relative environment paths from cwd, independently of projectPath", () => {
    vi.stubEnv("UTOO_ROOT_PATH", "..");

    expect(resolveRootPath(projectPath)).toBe(path.resolve(".."));
  });

  it.each(["absolute", "relative"])(
    "uses the %s environment root for integrations that call findRootDir directly",
    (kind) => {
      const rootPath =
        kind === "absolute" ? fixture : path.relative(process.cwd(), fixture);
      vi.stubEnv("UTOO_ROOT_PATH", rootPath);

      expect(findRootDir(projectPath)).toBe(fixture);
      expect(resolveRootPath(projectPath, projectPath)).toBe(fixture);
    },
  );

  it.each([undefined, ""])(
    "preserves the configured root when the environment is %s",
    (value) => {
      vi.stubEnv("UTOO_ROOT_PATH", value);

      expect(resolveRootPath(projectPath, fixture)).toBe(fixture);
    },
  );

  it.each([undefined, ""])(
    "detects the project root when the environment is %s and no root is configured",
    (value) => {
      vi.stubEnv("UTOO_ROOT_PATH", value);

      expect(resolveRootPath(projectPath)).toBe(projectPath);
    },
  );

  it("falls back to detecting the root from cwd when projectPath is omitted", () => {
    const cwd = vi.spyOn(process, "cwd").mockReturnValue(projectPath);
    try {
      expect(resolveRootPath()).toBe(projectPath);
    } finally {
      cwd.mockRestore();
    }
  });
});
