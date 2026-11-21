import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NapiIssue } from "../binding";
import {
  batchFailure,
  createEditBatchController,
  EditBatchBuildError,
  type EditBatchEvent,
  type EditBatchPolicy,
  EditBatchWaitTimeoutError,
} from "../core/editBatch";

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const explicit: EditBatchPolicy = {
  timeout: { action: "none" },
  afterBuild: "hold",
};
const issue: NapiIssue = {
  severity: "error",
  stage: "parse",
  filePath: "src/index.ts",
  title: { type: "text", value: "Expected expression" },
  description: { type: "text", value: "Unexpected token at line 3" },
  documentationLink: "",
  importTraces: [],
};

function setup(editBatch: EditBatchPolicy = explicit) {
  const backend = {
    assertSupported: vi.fn(() => {}),
    setWatchBatchHold: vi.fn(async (_held: boolean) => {}),
    build: vi.fn(async (_id: string) => {}),
    close: vi.fn(async () => {}),
  };
  const events: EditBatchEvent[] = [];
  const onEditBatchEvent = vi.fn((event: EditBatchEvent) => {
    events.push(event);
  });
  const controller = createEditBatchController(backend, {
    editBatch,
    onEditBatchEvent,
  });
  return { backend, events, onEditBatchEvent, controller };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("edit batch completion and hold policy", () => {
  it("returns one immutable result only after output and resume ACK", async () => {
    const {
      controller: c,
      backend,
      events,
    } = setup({ ...explicit, afterBuild: "resume" });
    const output = deferred();
    const resume = deferred();
    backend.build.mockImplementation(() => output.promise);
    backend.setWatchBatchHold.mockImplementation((held) =>
      held ? Promise.resolve() : resume.promise,
    );
    const id = await c.begin();
    const commit = c.commit(id);
    const wait = c.wait(id);
    expect(c.commit(id)).toBe(commit);
    await expect(c.begin()).rejects.toMatchObject({ code: "BATCH_BUSY" });
    output.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(events).toEqual([]);
    expect(c.activeId()).toBe(id);
    resume.resolve();
    const result = await commit;
    expect(result).toEqual({
      batchId: id,
      outcome: "built",
      reason: "commit",
      watchState: "watching",
    });
    expect(await wait).toBe(result);
    expect(await c.wait(id)).toBe(result);
    expect(await c.commit(id)).toBe(result);
    expect(Object.isFrozen(result)).toBe(true);
    expect(backend.build).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(events).toEqual([{ type: "settled", result }]);
  });

  it("none keeps the watcher held beyond 30 seconds and reuses hold for the next batch", async () => {
    const { controller: c, backend } = setup();
    const a = await c.begin();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(c.activeId()).toBe(a);
    expect(backend.build).not.toHaveBeenCalled();
    expect(await c.commit(a)).toMatchObject({
      outcome: "built",
      watchState: "held",
    });
    const b = await c.begin();
    expect(b).not.toBe(a);
    expect(backend.setWatchBatchHold).toHaveBeenCalledTimes(1);
    await c.commit(b);
  });

  it("keeps a failed result after a different batch repairs the source", async () => {
    const { controller: c, backend } = setup();
    backend.build.mockRejectedValueOnce(
      new EditBatchBuildError(
        batchFailure("COMPILATION_FAILED", new Error("parse failed"), [issue]),
      ),
    );
    const a = await c.begin();
    const failed = await c.commit(a);
    expect(failed).toMatchObject({
      outcome: "failed",
      watchState: "held",
      errors: [{ code: "COMPILATION_FAILED", issues: [issue] }],
    });
    const b = await c.begin();
    expect(await c.commit(b)).toMatchObject({ outcome: "built" });
    expect(await c.wait(a)).toBe(failed);
  });

  it("retains compilation diagnostics when resuming also fails", async () => {
    const { controller: c, backend } = setup({
      ...explicit,
      afterBuild: "resume",
    });
    backend.build.mockRejectedValue(
      new EditBatchBuildError(
        batchFailure("COMPILATION_FAILED", new Error("parse failed"), [issue]),
      ),
    );
    backend.setWatchBatchHold.mockImplementation(async (held) => {
      if (!held) throw new Error("control channel closed");
    });
    const id = await c.begin();
    expect(await c.commit(id)).toMatchObject({
      outcome: "failed",
      watchState: "unknown",
      errors: [
        { code: "COMPILATION_FAILED", issues: [issue] },
        { code: "WATCH_CONTROL_FAILED", message: "control channel closed" },
      ],
    });
    await expect(c.begin()).rejects.toMatchObject({
      code: "WATCH_STATE_UNKNOWN",
    });
  });

  it("does not infer held from policy when flush loses the native control connection", async () => {
    const { controller: c, backend } = setup();
    backend.build.mockRejectedValue(
      new EditBatchBuildError(
        batchFailure(
          "WATCH_CONTROL_FAILED",
          new Error("flush ACK disconnected"),
        ),
      ),
    );
    expect(await c.commit(await c.begin())).toMatchObject({
      outcome: "failed",
      watchState: "unknown",
    });
    await expect(c.begin()).rejects.toMatchObject({
      code: "WATCH_STATE_UNKNOWN",
    });
    expect(backend.setWatchBatchHold).toHaveBeenCalledTimes(1);
  });

  it("rejects unsupported configuration and invalid policy before installing hold", async () => {
    const { controller: c, backend } = setup();
    backend.assertSupported.mockImplementation(() => {
      throw new Error("lazy imports unsupported");
    });
    await expect(c.begin()).rejects.toThrow("lazy imports unsupported");
    expect(backend.setWatchBatchHold).not.toHaveBeenCalled();
    for (const afterMs of [0, -1, 1.5, NaN, Infinity, 2_147_483_648]) {
      expect(() =>
        createEditBatchController(backend, {
          editBatch: {
            timeout: { action: "release", afterMs },
            afterBuild: "hold",
          },
        }),
      ).toThrow("Invalid editBatch");
    }
  });
});

describe("hold timeout ownership", () => {
  it("defaults to release after 30 seconds, returning expired rather than built", async () => {
    const { backend } = setup();
    const c = createEditBatchController(backend);
    const id = await c.begin();
    await vi.advanceTimersByTimeAsync(29_999);
    expect(c.activeId()).toBe(id);
    await vi.advanceTimersByTimeAsync(1);
    expect(await c.wait(id)).toMatchObject({
      outcome: "expired",
      reason: "timeout",
      watchState: "watching",
      error: { code: "HOLD_TIMEOUT_RELEASED" },
    });
    expect(backend.build).not.toHaveBeenCalled();
  });

  it("only starts the hold timer after native acknowledgement", async () => {
    const { controller: c, backend } = setup({
      timeout: { action: "release", afterMs: 10 },
      afterBuild: "hold",
    });
    const hold = deferred();
    backend.setWatchBatchHold.mockImplementationOnce(() => hold.promise);
    const begin = c.begin();
    await vi.advanceTimersByTimeAsync(100);
    expect(c.activeId()).toBeUndefined();
    hold.resolve();
    const id = await begin;
    await vi.advanceTimersByTimeAsync(9);
    expect(c.activeId()).toBe(id);
    await vi.advanceTimersByTimeAsync(1);
    expect(await c.wait(id)).toMatchObject({ outcome: "expired" });
  });

  it("keep-held reminds once and accepts a later commit under the same ID", async () => {
    const {
      controller: c,
      backend,
      events,
    } = setup({
      timeout: { action: "keep-held", afterMs: 10 },
      afterBuild: "hold",
    });
    const id = await c.begin();
    await vi.advanceTimersByTimeAsync(100);
    expect(events).toEqual([
      { type: "hold-timeout", batchId: id, action: "keep-held" },
    ]);
    expect(backend.setWatchBatchHold).toHaveBeenCalledTimes(1);
    expect(await c.commit(id)).toMatchObject({
      outcome: "built",
      watchState: "held",
    });
  });

  it("commit claiming first prevents the timer from releasing an unfinished build", async () => {
    const { controller: c, backend } = setup({
      timeout: { action: "release", afterMs: 10 },
      afterBuild: "hold",
    });
    const output = deferred();
    backend.build.mockImplementation(() => output.promise);
    const id = await c.begin();
    const commit = c.commit(id);
    await vi.advanceTimersByTimeAsync(100);
    expect(backend.setWatchBatchHold).toHaveBeenCalledTimes(1);
    output.resolve();
    expect(await commit).toMatchObject({
      outcome: "built",
      watchState: "held",
    });
  });

  it("timer claiming first makes commit join its release instead of building", async () => {
    const { controller: c, backend } = setup({
      timeout: { action: "release", afterMs: 10 },
      afterBuild: "hold",
    });
    const release = deferred();
    backend.setWatchBatchHold.mockImplementation((held) =>
      held ? Promise.resolve() : release.promise,
    );
    const id = await c.begin();
    await vi.advanceTimersByTimeAsync(10);
    const commit = c.commit(id);
    release.resolve();
    expect(await commit).toMatchObject({
      outcome: "expired",
      watchState: "watching",
    });
    expect(backend.build).not.toHaveBeenCalled();
    expect(backend.setWatchBatchHold.mock.calls).toEqual([[true], [false]]);
  });

  it("timer release failure is a timeout failure with unknown control state", async () => {
    const { controller: c, backend } = setup({
      timeout: { action: "release", afterMs: 10 },
      afterBuild: "resume",
    });
    backend.setWatchBatchHold.mockImplementation(async (held) => {
      if (!held) throw new Error("release ACK failed");
    });
    const id = await c.begin();
    await vi.advanceTimersByTimeAsync(10);
    expect(await c.commit(id)).toMatchObject({
      outcome: "failed",
      reason: "timeout",
      watchState: "unknown",
      errors: [{ code: "WATCH_CONTROL_FAILED", message: "release ACK failed" }],
    });
    await expect(c.begin()).rejects.toMatchObject({
      code: "WATCH_STATE_UNKNOWN",
    });
    expect(backend.build).not.toHaveBeenCalled();
  });
});

describe("waiters, history and observers", () => {
  it("wait timeout and abort do not change a pending commit or release hold", async () => {
    const { controller: c, backend } = setup();
    const output = deferred();
    backend.build.mockImplementation(() => output.promise);
    const id = await c.begin();
    const commit = c.commit(id);
    const abort = new AbortController();
    const aborted = c
      .wait(id, { signal: abort.signal })
      .catch((error: unknown) => error);
    const timed = c
      .wait(id, { timeoutMs: 10 })
      .catch((error: unknown) => error);
    abort.abort(new Error("caller disconnected"));
    await vi.advanceTimersByTimeAsync(10);
    expect(await timed).toBeInstanceOf(EditBatchWaitTimeoutError);
    expect(await aborted).toMatchObject({ message: "caller disconnected" });
    expect(c.activeId()).toBe(id);
    expect(backend.setWatchBatchHold).toHaveBeenCalledTimes(1);
    output.resolve();
    expect(await c.wait(id)).toBe(await commit);
  });

  it("a throwing or rejecting observer cannot change a stored result", async () => {
    const { backend } = setup();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const c = createEditBatchController(backend, {
      editBatch: explicit,
      onEditBatchEvent: async () => {
        throw new Error("observer unavailable");
      },
    });
    const id = await c.begin();
    const result = await c.commit(id);
    await vi.advanceTimersByTimeAsync(0);
    expect(await c.wait(id)).toBe(result);
    expect(result.outcome).toBe("built");
    expect(error).toHaveBeenCalledWith(
      "Edit batch observer failed",
      expect.any(Error),
    );
  });

  it("bounds history to 32 and never rebuilds an evicted or unknown ID", async () => {
    const { controller: c, backend } = setup();
    const first = await c.begin();
    await c.commit(first);
    for (let i = 0; i < 32; i++) await c.commit(await c.begin());
    expect(() => c.wait(first)).toThrow(/Unknown or evicted/);
    expect(() => c.commit("unknown")).toThrow(/Unknown or evicted/);
    expect(backend.build).toHaveBeenCalledTimes(33);
  });

  it("preserves bounded aggregate cause chains and error paths", () => {
    const io = Object.assign(new Error("cannot write output"), {
      code: "EACCES",
      path: "/output/main.js",
    });
    const other = new Error("HTML failed", {
      cause: new Error("bad template"),
    });
    const value = batchFailure(
      "OUTPUT_WRITE_FAILED",
      new AggregateError([io, other], "multiple output errors"),
    );
    expect(value.causes).toContainEqual({
      name: "Error",
      message: "cannot write output",
      code: "EACCES",
      path: "/output/main.js",
    });
    expect(value.causes).toContainEqual({
      name: "Error",
      message: "bad template",
    });
    let nested: Error = new Error("root");
    for (let i = 0; i < 20; i++)
      nested = new Error(`level-${i}`, { cause: nested });
    expect(batchFailure("OUTPUT_WRITE_FAILED", nested).causes).toHaveLength(8);
    const plain = {
      name: "SystemError",
      message: "disk is read-only",
      code: "EROFS",
      path: "/dist/main.js",
    };
    expect(batchFailure("OUTPUT_WRITE_FAILED", plain)).toMatchObject({
      message: plain.message,
      causes: [plain],
    });
  });
});

describe("close owns only an unrecorded terminal result", () => {
  it.each(["holding", "building", "releasing"] as const)(
    "close wins during %s without releasing drafts or accepting late built",
    async (phase) => {
      const {
        controller: c,
        backend,
        events,
      } = setup({
        timeout: { action: "release", afterMs: 10 },
        afterBuild: "hold",
      });
      const output = deferred();
      const shutdown = deferred();
      backend.build.mockImplementation(() => output.promise);
      backend.close.mockImplementation(() => shutdown.promise);
      if (phase === "releasing")
        backend.setWatchBatchHold.mockImplementation((held) =>
          held ? Promise.resolve() : output.promise,
        );
      const id = await c.begin();
      if (phase === "building") c.commit(id);
      if (phase === "releasing") await vi.advanceTimersByTimeAsync(10);
      const close = c.close();
      output.resolve();
      await vi.advanceTimersByTimeAsync(0);
      expect(events.filter((e) => e.type === "settled")).toEqual([]);
      shutdown.resolve();
      await close;
      expect(await c.wait(id)).toMatchObject({
        outcome: "failed",
        reason: "close",
        watchState: "closed",
        errors: [{ code: "SERVER_CLOSED" }],
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(events.filter((e) => e.type === "settled")).toHaveLength(1);
      if (phase !== "releasing")
        expect(backend.setWatchBatchHold.mock.calls).toEqual([[true]]);
    },
  );

  it("reports close failure with its actual cause and keeps an unconfirmed close pending", async () => {
    const { controller: c, backend } = setup();
    const shutdown = deferred();
    backend.close.mockImplementation(() => shutdown.promise);
    const id = await c.begin();
    const close = c.close().catch((error: unknown) => error);
    const wait = c.wait(id, { timeoutMs: 10 }).catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(10);
    expect(await wait).toBeInstanceOf(EditBatchWaitTimeoutError);
    const cause = Object.assign(new Error("shutdown flush failed"), {
      code: "EIO",
    });
    shutdown.reject(cause);
    expect(await close).toBe(cause);
    expect(await c.wait(id)).toMatchObject({
      outcome: "failed",
      reason: "close",
      watchState: "unknown",
      errors: [
        {
          code: "CLOSE_FAILED",
          causes: [{ code: "EIO", message: "shutdown flush failed" }],
        },
      ],
    });
  });

  it("retains a recorded result when a later server close fails", async () => {
    const { controller: c, backend } = setup();
    const id = await c.begin();
    const built = await c.commit(id);
    backend.close.mockRejectedValue(new Error("later close failure"));
    await expect(c.close()).rejects.toThrow("later close failure");
    expect(await c.wait(id)).toBe(built);
  });
});
