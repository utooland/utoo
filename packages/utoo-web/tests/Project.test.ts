import * as comlink from "comlink";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Project } from "../src/project/Project";

class MockWorker extends EventTarget {
  static instances: MockWorker[] = [];
  static dispose = async () => {};

  readonly messages: unknown[] = [];
  terminateCalls = 0;

  constructor(_url: string | URL, _options?: WorkerOptions) {
    super();
    MockWorker.instances.push(this);
  }

  postMessage(message: unknown, transfer?: Transferable[]): void {
    this.messages.push(message);

    const port = transfer?.[0];
    if (port instanceof MessagePort) {
      comlink.expose(
        {
          mount: async () => {},
          install: () => new Promise<void>(() => {}),
          build: () => new Promise<void>(() => {}),
          dispose: () => MockWorker.dispose(),
        },
        port,
      );
    }
  }

  terminate(): void {
    this.terminateCalls += 1;
  }
}

function createProject(): Project {
  return new Project({
    cwd: "/project",
    workerUrl: "/worker.js",
    threadWorkerUrl: "/thread-worker.js",
  });
}

let project: Project | undefined;

beforeEach(() => {
  MockWorker.instances = [];
  MockWorker.dispose = async () => {};
  vi.stubGlobal("Worker", MockWorker);
  vi.stubGlobal("window", new EventTarget());
});

afterEach(async () => {
  await project?.dispose();
  project = undefined;
  vi.unstubAllGlobals();
});

describe("Project.dispose", () => {
  it("aborts active install and build calls", async () => {
    project = createProject();
    await project.mount();

    const install = project.install("{}");
    const build = project.build();
    const disposal = project.dispose();

    await expect(install).rejects.toMatchObject({ name: "AbortError" });
    await expect(build).rejects.toMatchObject({ name: "AbortError" });
    await disposal;
    expect(MockWorker.instances[0].terminateCalls).toBe(1);
  });

  it("is idempotent and starts a fresh worker for the next project", async () => {
    project = createProject();
    const firstWorker = MockWorker.instances[0];

    await Promise.all([project.dispose(), project.dispose()]);

    expect(firstWorker.terminateCalls).toBe(1);
    await expect(project.mount()).rejects.toMatchObject({ name: "AbortError" });

    project = createProject();
    expect(MockWorker.instances).toHaveLength(2);
    expect(MockWorker.instances[1]).not.toBe(firstWorker);
  });

  it("waits for Rust disposal before terminating the worker", async () => {
    let finishRustDisposal: (() => void) | undefined;
    MockWorker.dispose = () =>
      new Promise<void>((resolve) => {
        finishRustDisposal = resolve;
      });
    project = createProject();
    await project.mount();

    const disposal = project.dispose();
    await vi.waitFor(() => expect(finishRustDisposal).toBeDefined());

    expect(MockWorker.instances[0].terminateCalls).toBe(0);
    finishRustDisposal!();
    await disposal;
    expect(MockWorker.instances[0].terminateCalls).toBe(1);
  });
});
