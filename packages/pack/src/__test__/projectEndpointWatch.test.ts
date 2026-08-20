import { beforeEach, describe, expect, it, vi } from "vitest";

const bindingMocks = vi.hoisted(() => ({
  endpointClientChangedSubscribe: vi.fn(),
  endpointServerChangedSubscribe: vi.fn(),
  projectEntrypointsSubscribe: vi.fn(),
  projectNew: vi.fn(),
  registerWorkerScheduler: undefined,
  rootTaskDispose: vi.fn(),
}));

vi.mock("../binding", () => bindingMocks);

import { projectFactory } from "../core/project";

function emitOnce(
  callback: (error: Error | undefined, value: unknown) => void,
) {
  callback(undefined, { issues: [] });
  return Promise.resolve();
}

async function createProject(hasServerOutput: boolean, nodeTarget: boolean) {
  const nativeProject = { __napiType: "Project" as const };
  bindingMocks.projectNew.mockResolvedValue(nativeProject);
  const project = await projectFactory({ hasServerOutput, nodeTarget })(
    {
      config: { entry: [], output: {} },
      dev: true,
      watch: { enable: true },
    } as never,
    {} as never,
  );
  const first = { __napiType: "Endpoint" as const };
  const second = { __napiType: "Endpoint" as const };
  bindingMocks.projectEntrypointsSubscribe.mockImplementationOnce(
    (_project, callback) => {
      callback(undefined, { apps: [first, second], issues: [] });
      return Promise.resolve();
    },
  );
  const entrypoints = await project.entrypointsSubscribe().next();
  return entrypoints.value.apps!;
}

beforeEach(() => {
  vi.clearAllMocks();
  bindingMocks.endpointClientChangedSubscribe.mockImplementation(
    (_endpoint, callback) => emitOnce(callback),
  );
  bindingMocks.endpointServerChangedSubscribe.mockImplementation(
    (_endpoint, _issues, callback) => emitOnce(callback),
  );
});

describe("endpoint watch subscriptions", () => {
  it("creates only client subscriptions for browser apps without server output", async () => {
    const endpoints = await createProject(false, false);

    await Promise.all(
      endpoints.flatMap((endpoint) => [
        endpoint.clientChanged(),
        endpoint.serverChanged(true),
      ]),
    );

    expect(bindingMocks.endpointClientChangedSubscribe).toHaveBeenCalledTimes(
      2,
    );
    expect(bindingMocks.endpointServerChangedSubscribe).not.toHaveBeenCalled();
  });

  it("shares one server subscription across browser app endpoints", async () => {
    const endpoints = await createProject(true, false);

    await Promise.all(
      endpoints.map((endpoint) => endpoint.serverChanged(true)),
    );

    expect(bindingMocks.endpointServerChangedSubscribe).toHaveBeenCalledTimes(
      1,
    );
  });

  it("creates only server subscriptions for Node endpoints", async () => {
    const endpoints = await createProject(false, true);

    await Promise.all(
      endpoints.flatMap((endpoint) => [
        endpoint.clientChanged(),
        endpoint.serverChanged(true),
      ]),
    );

    expect(bindingMocks.endpointClientChangedSubscribe).not.toHaveBeenCalled();
    expect(bindingMocks.endpointServerChangedSubscribe).toHaveBeenCalledTimes(
      2,
    );
  });
});
