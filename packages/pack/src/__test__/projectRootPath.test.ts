import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const bindingMocks = vi.hoisted(() => ({
  projectNew: vi.fn(),
  projectUpdate: vi.fn(),
  registerWorkerScheduler: undefined,
}));

vi.mock("../binding", () => bindingMocks);

import { projectFactory } from "../core/project";
import { normalizePath } from "../utils/normalizePath";

const nativeProject = { __napiType: "Project" as const };
let fixture: string;
let projectPath: string;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubEnv("UTOO_ROOT_PATH", undefined);
  bindingMocks.projectNew.mockResolvedValue(nativeProject);
  bindingMocks.projectUpdate.mockResolvedValue(undefined);
  fixture = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-project-root-"));
  projectPath = path.join(fixture, "worktree", "app");
  fs.mkdirSync(projectPath, { recursive: true });
  fs.writeFileSync(path.join(fixture, "package-lock.json"), "{}");
});

afterEach(() => {
  vi.unstubAllEnvs();
  fs.rmSync(fixture, { recursive: true, force: true });
});

function createProject(rootPath?: string) {
  return projectFactory()(
    {
      projectPath,
      rootPath,
      config: { entry: [] },
      processEnv: {},
      watch: { enable: false },
      dev: false,
      buildId: "root-path-test",
      tracing: false,
      packPath: fixture,
    },
    {},
  );
}

function expectNativeRoot(rootPath: string) {
  expect(bindingMocks.projectNew).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({
      projectPath: normalizePath(projectPath),
      rootPath: normalizePath(rootPath),
    }),
    {},
    expect.any(Object),
  );
}

describe("project root configuration", () => {
  it("overrides an explicit root before passing options to NAPI", async () => {
    vi.stubEnv("UTOO_ROOT_PATH", fixture);

    await createProject(projectPath);

    expectNativeRoot(fixture);
  });

  it("resolves relative environment roots from cwd", async () => {
    vi.stubEnv("UTOO_ROOT_PATH", path.relative(process.cwd(), fixture));

    await createProject(projectPath);

    expectNativeRoot(fixture);
  });

  it.each([undefined, ""])(
    "preserves an explicit root when the environment is %s",
    async (value) => {
      vi.stubEnv("UTOO_ROOT_PATH", value);

      await createProject(projectPath);

      expectNativeRoot(projectPath);
    },
  );

  it.each([undefined, ""])(
    "detects the root for direct project creation when the environment is %s",
    async (value) => {
      vi.stubEnv("UTOO_ROOT_PATH", value);

      await createProject();

      expectNativeRoot(fixture);
    },
  );

  it("preserves the existing root during config-only updates", async () => {
    const project = await createProject(projectPath);
    vi.stubEnv("UTOO_ROOT_PATH", fixture);

    await project.update({ config: { entry: [] } });

    expect(bindingMocks.projectUpdate).toHaveBeenCalledExactlyOnceWith(
      nativeProject,
      expect.objectContaining({ rootPath: undefined }),
    );
  });

  it("applies the environment override to explicit root updates", async () => {
    const project = await createProject(projectPath);
    vi.stubEnv("UTOO_ROOT_PATH", fixture);

    await project.update({ rootPath: projectPath });

    expect(bindingMocks.projectUpdate).toHaveBeenCalledExactlyOnceWith(
      nativeProject,
      expect.objectContaining({ rootPath: normalizePath(fixture) }),
    );
  });

  it("preserves explicit root updates without an environment override", async () => {
    const project = await createProject(projectPath);

    await project.update({ rootPath: fixture });

    expect(bindingMocks.projectUpdate).toHaveBeenCalledExactlyOnceWith(
      nativeProject,
      expect.objectContaining({ rootPath: normalizePath(fixture) }),
    );
  });
});
