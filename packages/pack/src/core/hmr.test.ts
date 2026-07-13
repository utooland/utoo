import type { Issue } from "@utoo/pack-shared";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Endpoint, Project } from "./types";

let mockProject: Project;
let mockEndpoint: Endpoint;

vi.mock("./project", () => ({
  projectFactory: () => async () => mockProject,
}));

function createIssue(): Issue {
  return {
    severity: "error",
    stage: "build",
    filePath: "[project]/src/index.less",
    title: {
      type: "text",
      value: "Unrecognised input",
    },
    description: {
      type: "text",
      value: "Less parser failed",
    },
    documentationLink: "",
    importTraces: [],
  };
}

async function* once<T>(value: T) {
  yield value;
}

async function* empty<T>() {}

describe("createHotReloader", () => {
  beforeEach(() => {
    mockEndpoint = {
      writeToDisk: vi.fn(async () => ({
        issues: [createIssue()],
      })) as any,
      clientChanged: vi.fn(async () => empty<TurbopackResult>()),
      serverChanged: vi.fn(async () => empty<TurbopackResult>()),
    };

    mockProject = {
      update: vi.fn(),
      writeAllEntrypointsToDisk: vi.fn(async () => ({
        apps: [mockEndpoint],
        appPaths: [],
        issues: [createIssue()],
      })),
      entrypointsSubscribe: vi.fn(() =>
        once({
          apps: [mockEndpoint],
          issues: [],
        }),
      ),
      hmrEvents: vi.fn(),
      hmrIdentifiersSubscribe: vi.fn(),
      getSourceForAsset: vi.fn(),
      getSourceMap: vi.fn(),
      getSourceMapSync: vi.fn(),
      traceSource: vi.fn(),
      updateInfoSubscribe: vi.fn(() => empty()),
      shutdown: vi.fn(async () => {}),
      onExit: vi.fn(async () => {}),
    };
  });

  async function createStartedHotReloader(
    stats = false,
    onCompileDone = vi.fn(),
  ) {
    const { createHotReloader } = await import("./hmr");
    const hotReloader = await createHotReloader(
      {
        config: {
          entry: [],
          output: {
            clean: false,
            path: "dist",
          },
          optimization: {},
          persistentCaching: false,
          stats,
        },
      } as any,
      undefined,
      undefined,
      { onCompileDone },
    );

    await hotReloader.start();
    return { hotReloader, onCompileDone };
  }

  function collectSyncErrors(
    hotReloader: Awaited<
      ReturnType<typeof createStartedHotReloader>
    >["hotReloader"],
  ) {
    const sent: any[] = [];
    hotReloader.registerClient({
      send(data) {
        sent.push(JSON.parse(data));
      },
      close: vi.fn(),
    });

    const sync = sent.find((message) => message.action === "sync");
    return sync?.errors;
  }

  it("surfaces initial endpoint output issues through sync without failing startup", async () => {
    const { hotReloader, onCompileDone } = await createStartedHotReloader();

    const errors = collectSyncErrors(hotReloader);
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toContain("Less parser failed");
    expect(onCompileDone).toHaveBeenCalledWith({
      errors: expect.arrayContaining([
        expect.objectContaining({
          message: expect.stringContaining("Less parser failed"),
        }),
      ]),
      warnings: [],
    });
    expect(mockEndpoint.writeToDisk).toHaveBeenCalled();

    await hotReloader.close();
  });

  it("surfaces initial project output issues through sync without failing startup", async () => {
    const { hotReloader, onCompileDone } = await createStartedHotReloader(true);

    const errors = collectSyncErrors(hotReloader);
    expect(errors).toHaveLength(1);
    expect(errors[0].message).toContain("Less parser failed");
    expect(onCompileDone).toHaveBeenCalledWith({
      errors: expect.arrayContaining([
        expect.objectContaining({
          message: expect.stringContaining("Less parser failed"),
        }),
      ]),
      warnings: [],
    });
    expect(mockProject.writeAllEntrypointsToDisk).toHaveBeenCalled();

    await hotReloader.close();
  });
});
