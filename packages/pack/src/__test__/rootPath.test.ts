import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const rootMocks = vi.hoisted(() => {
  const stopped = new Error("Stop before starting the bundler");
  return {
    stopped,
    createProject: vi.fn(async () => {
      throw stopped;
    }),
    createHotReloader: vi.fn(async () => {
      throw stopped;
    }),
    resolveBundleOptions: vi.fn(() => ({
      config: { entry: [], persistentCaching: false },
    })),
  };
});

vi.mock("../core/project", () => ({
  projectFactory: () => rootMocks.createProject,
}));
vi.mock("../core/hmr", () => ({
  createHotReloader: rootMocks.createHotReloader,
}));
vi.mock("../config/webpackCompat", () => ({
  resolveBundleOptions: rootMocks.resolveBundleOptions,
}));
vi.mock("../utils/common", () => ({
  blockStdout: vi.fn(),
  getPackPath: () => "/fixture/pack",
}));
vi.mock("../utils/cleanOutput", () => ({
  cleanOutput: vi.fn(async () => {}),
  getOutputPath: vi.fn(),
}));
vi.mock("../utils/lockfile", () => ({
  acquirePersistentCacheLock: vi.fn(async () => undefined),
}));
vi.mock("get-port", () => ({ default: async () => 3000 }));
vi.mock("domparser-rs", () => ({ DOMParser: vi.fn() }));

import { build } from "../commands/build";
import { serve } from "../commands/dev";

const projectPath = path.resolve("/fixture/worktree/app");
const configuredRoot = path.resolve("/fixture/worktree");
const environmentRoot = path.resolve("/fixture");
const originalTitle = process.title;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubEnv("UTOO_ROOT_PATH", undefined);
  vi.stubEnv("XCODE_PROFILE", undefined);
  vi.stubEnv("NODE_ENV", process.env.NODE_ENV);
});

afterEach(() => {
  vi.unstubAllEnvs();
  process.title = originalTitle;
});

describe.each([
  ["build", build],
  ["serve", serve],
] as const)("%s root path", (name, command) => {
  async function expectRoot(
    expectedRoot: string,
    options: Parameters<typeof build>[0] = { config: { entry: [] } },
  ) {
    await expect(command(options, projectPath, configuredRoot)).rejects.toBe(
      rootMocks.stopped,
    );

    if (name === "build") {
      expect(rootMocks.createProject).toHaveBeenCalledExactlyOnceWith(
        expect.objectContaining({ rootPath: expectedRoot }),
        expect.any(Object),
      );
    } else {
      expect(rootMocks.createHotReloader).toHaveBeenCalledExactlyOnceWith(
        expect.any(Object),
        projectPath,
        expectedRoot,
      );
    }
  }

  it("passes the environment root instead of the configured root", async () => {
    vi.stubEnv("UTOO_ROOT_PATH", environmentRoot);

    await expectRoot(environmentRoot);
  });

  it("preserves the configured root when the environment is unset", async () => {
    await expectRoot(configuredRoot);
  });

  it("loads webpack configuration using the original root", async () => {
    vi.stubEnv("UTOO_ROOT_PATH", environmentRoot);
    const options = { webpackMode: true };

    await expectRoot(environmentRoot, options);

    expect(rootMocks.resolveBundleOptions).toHaveBeenCalledExactlyOnceWith(
      options,
      projectPath,
      configuredRoot,
    );
  });
});
