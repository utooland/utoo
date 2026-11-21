import { randomUUID } from "node:crypto";
import type { NapiIssue } from "../binding";

export type EditBatchTimeoutPolicy =
  | { action: "none" }
  | { action: "release" | "keep-held"; afterMs: number };

export interface EditBatchPolicy {
  timeout: EditBatchTimeoutPolicy;
  afterBuild: "resume" | "hold";
}

export interface EditBatchWaitOptions {
  timeoutMs?: number;
  signal?: AbortSignal;
}

export type EditBatchErrorCode =
  | "COMPILATION_FAILED"
  | "OUTPUT_WRITE_FAILED"
  | "HTML_GENERATION_FAILED"
  | "PUBLISH_CALLBACK_FAILED"
  | "WATCH_CONTROL_FAILED"
  | "CLOSE_FAILED"
  | "SERVER_CLOSED";

export interface EditBatchFailure {
  code: EditBatchErrorCode;
  message: string;
  issues: readonly NapiIssue[];
  causes: readonly {
    name: string;
    message: string;
    code?: string;
    path?: string;
  }[];
}

export type EditBatchResult =
  | {
      batchId: string;
      outcome: "built";
      reason: "commit";
      watchState: "held" | "watching";
    }
  | {
      batchId: string;
      outcome: "failed";
      reason: "commit" | "timeout" | "close";
      watchState: "held" | "watching" | "closed" | "unknown";
      errors: readonly [EditBatchFailure, ...EditBatchFailure[]];
    }
  | {
      batchId: string;
      outcome: "expired";
      reason: "timeout";
      watchState: "watching";
      error: { code: "HOLD_TIMEOUT_RELEASED"; message: string };
    };

export type EditBatchEvent =
  | { type: "hold-timeout"; batchId: string; action: "release" | "keep-held" }
  | { type: "settled"; result: EditBatchResult };

const requestStatus = {
  BATCH_BUSY: 409,
  BATCH_NOT_FOUND: 404,
  SERVER_CLOSING: 503,
  WATCH_STATE_UNKNOWN: 503,
  WATCH_CONTROL_FAILED: 503,
  INVALID_BATCH_POLICY: 400,
  INVALID_WAIT_OPTIONS: 400,
  UNSUPPORTED_BATCH_CONFIGURATION: 409,
} as const;

export class EditBatchRequestError extends Error {
  readonly status: (typeof requestStatus)[keyof typeof requestStatus];

  constructor(
    readonly code: keyof typeof requestStatus,
    message: string,
    readonly failure?: EditBatchFailure,
  ) {
    super(message);
    this.name = "EditBatchRequestError";
    this.status = requestStatus[code];
  }
}

export class EditBatchWaitTimeoutError extends Error {
  constructor(readonly batchId: string) {
    super(`Edit batch ${batchId} is still pending`);
    this.name = "EditBatchWaitTimeoutError";
  }
}

/** Carries the original diagnostics through the existing output pipeline. */
export class EditBatchBuildError extends Error {
  constructor(readonly failure: EditBatchFailure) {
    super(failure.message);
    this.name = "EditBatchBuildError";
  }
}

export function batchFailure(
  code: EditBatchErrorCode,
  error: unknown,
  issues: readonly NapiIssue[] = [],
): EditBatchFailure {
  const causes: EditBatchFailure["causes"][number][] = [];
  const seen = new Set<unknown>();
  function describe(value: unknown): EditBatchFailure["causes"][number] {
    if (value !== null && typeof value === "object") {
      return {
        name:
          "name" in value && typeof value.name === "string"
            ? value.name
            : "Error",
        message:
          "message" in value && typeof value.message === "string"
            ? value.message
            : String(value),
        ...("code" in value && typeof value.code === "string"
          ? { code: value.code }
          : {}),
        ...("path" in value && typeof value.path === "string"
          ? { path: value.path }
          : {}),
      };
    }
    return { name: "Error", message: String(value) };
  }
  function visit(value: unknown, depth: number) {
    if (depth >= 8 || seen.has(value)) return;
    seen.add(value);
    causes.push(describe(value));
    if (value !== null && typeof value === "object") {
      if (value instanceof AggregateError && Array.isArray(value.errors)) {
        for (const child of value.errors as unknown[]) visit(child, depth + 1);
      }
      if ("cause" in value && value.cause !== undefined)
        visit(value.cause, depth + 1);
    }
  }
  visit(error, 0);
  return { code, message: describe(error).message, issues, causes };
}

const MAX_TIMER_MS = 2_147_483_647;
const validTimeout = (ms: number) =>
  Number.isInteger(ms) && ms > 0 && ms <= MAX_TIMER_MS;

export function resolveEditBatchPolicy(
  policy?: EditBatchPolicy,
): EditBatchPolicy {
  const value = policy ?? {
    timeout: { action: "release", afterMs: 30_000 },
    afterBuild: "resume",
  };
  if (
    !value ||
    !value.timeout ||
    (value.afterBuild !== "resume" && value.afterBuild !== "hold") ||
    (value.timeout.action !== "none" &&
      ((value.timeout.action !== "release" &&
        value.timeout.action !== "keep-held") ||
        !validTimeout(value.timeout.afterMs)))
  ) {
    throw new EditBatchRequestError(
      "INVALID_BATCH_POLICY",
      "Invalid editBatch timeout or afterBuild policy",
    );
  }
  return Object.freeze({
    ...value,
    timeout: Object.freeze({ ...value.timeout }),
  });
}

interface EditBatchBackend {
  assertSupported(): void;
  setWatchBatchHold(held: boolean): Promise<void>;
  /** Claims the output owner synchronously, then flushes and publishes while held. */
  build(batchId: string): Promise<void>;
  /** Closes the actual HTTP, websocket and project resources, without releasing drafts. */
  close(): Promise<void>;
}

interface BatchRecord {
  id: string;
  phase: "holding" | "committing" | "releasing" | "settled";
  ready: boolean;
  timer?: NodeJS.Timeout;
  result?: EditBatchResult;
  errors: EditBatchFailure[];
  completion: Promise<EditBatchResult>;
  resolve(result: EditBatchResult): void;
  waiters: Set<(result: EditBatchResult) => void>;
}

function freezeResult(result: EditBatchResult): EditBatchResult {
  const copy = structuredClone(result);
  const seen = new WeakSet<object>();
  function freeze(value: unknown) {
    if (!value || typeof value !== "object" || seen.has(value)) return;
    seen.add(value);
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  freeze(copy);
  return copy;
}

export function createEditBatchController(
  backend: EditBatchBackend,
  options?: {
    editBatch?: EditBatchPolicy;
    onEditBatchEvent?: (event: EditBatchEvent) => void | Promise<void>;
  },
) {
  const policy = resolveEditBatchPolicy(options?.editBatch);
  let active: BatchRecord | undefined;
  const recent = new Map<string, BatchRecord>();
  let watchState: EditBatchResult["watchState"] = "watching";
  let closing = false;
  let closePromise: Promise<void> | undefined;

  function notify(event: EditBatchEvent) {
    void Promise.resolve()
      .then(() => options?.onEditBatchEvent?.(event))
      .catch((error: unknown) => {
        console.error("Edit batch observer failed", error);
      });
  }

  function store(batch: BatchRecord, result: EditBatchResult) {
    if (batch.result) return batch.result;
    clearTimeout(batch.timer);
    batch.result = freezeResult(result);
    batch.phase = "settled";
    if (active === batch) active = undefined;
    recent.set(batch.id, batch);
    while (recent.size > 32) recent.delete(recent.keys().next().value!);
    batch.resolve(batch.result);
    for (const waiter of batch.waiters) waiter(batch.result);
    batch.waiters.clear();
    notify({ type: "settled", result: batch.result });
    return batch.result;
  }

  function fail(batch: BatchRecord, reason: "commit" | "timeout" | "close") {
    const [first, ...rest] = batch.errors;
    if (!first) throw new Error("A failed edit batch must retain its cause");
    return store(batch, {
      batchId: batch.id,
      outcome: "failed",
      reason,
      watchState,
      errors: [first, ...rest],
    });
  }

  function lookup(id: string) {
    const batch = active?.id === id ? active : recent.get(id);
    if (!batch)
      throw new EditBatchRequestError(
        "BATCH_NOT_FOUND",
        `Unknown or evicted edit batch ${id}`,
      );
    return batch;
  }

  function checkOpen() {
    if (closing)
      throw new EditBatchRequestError(
        "SERVER_CLOSING",
        "Dev server is closing or closed",
      );
    if (watchState === "unknown")
      throw new EditBatchRequestError(
        "WATCH_STATE_UNKNOWN",
        "Watcher state is unknown; close this dev server before starting another batch",
      );
  }

  async function expire(batch: BatchRecord) {
    if (closing || active !== batch || batch.phase !== "holding") return;
    const timeout = policy.timeout;
    if (timeout.action === "none") return;
    if (timeout.action === "keep-held") {
      notify({
        type: "hold-timeout",
        batchId: batch.id,
        action: timeout.action,
      });
      return;
    }
    // Claim before yielding: a concurrent commit joins this result instead of flushing.
    batch.phase = "releasing";
    notify({ type: "hold-timeout", batchId: batch.id, action: timeout.action });
    try {
      await backend.setWatchBatchHold(false);
      if (closing) return;
      watchState = "watching";
      store(batch, {
        batchId: batch.id,
        outcome: "expired",
        reason: "timeout",
        watchState: "watching",
        error: {
          code: "HOLD_TIMEOUT_RELEASED",
          message:
            "Edit batch expired and the watcher was released; subsequent realtime output is not a committed batch",
        },
      });
    } catch (error) {
      if (closing) return;
      watchState = "unknown";
      batch.errors.push(batchFailure("WATCH_CONTROL_FAILED", error));
      fail(batch, "timeout");
    }
  }

  async function build(batch: BatchRecord) {
    try {
      await backend.build(batch.id);
    } catch (error) {
      if (closing) return;
      const failure =
        error instanceof EditBatchBuildError
          ? error.failure
          : batchFailure("OUTPUT_WRITE_FAILED", error);
      batch.errors.push(failure);
      if (failure.code === "WATCH_CONTROL_FAILED") watchState = "unknown";
    }
    if (closing) return;
    if (policy.afterBuild === "resume" && watchState !== "unknown") {
      try {
        await backend.setWatchBatchHold(false);
        if (closing) return;
        watchState = "watching";
      } catch (error) {
        if (closing) return;
        watchState = "unknown";
        batch.errors.push(batchFailure("WATCH_CONTROL_FAILED", error));
      }
    }
    if (closing) return;
    if (batch.errors.length) {
      fail(batch, "commit");
    } else if (watchState === "held" || watchState === "watching") {
      store(batch, {
        batchId: batch.id,
        outcome: "built",
        reason: "commit",
        watchState,
      });
    }
  }

  return {
    async begin() {
      checkOpen();
      if (active)
        throw new EditBatchRequestError(
          "BATCH_BUSY",
          "An edit batch is already active",
        );
      backend.assertSupported();
      let resolve!: BatchRecord["resolve"];
      const batch: BatchRecord = {
        id: randomUUID(),
        phase: "holding",
        ready: false,
        errors: [],
        waiters: new Set(),
        completion: new Promise((done) => {
          resolve = done;
        }),
        resolve: (result) => resolve(result),
      };
      active = batch;
      try {
        if (watchState !== "held") await backend.setWatchBatchHold(true);
      } catch (error) {
        if (!closing) {
          watchState = "unknown";
          active = undefined;
        }
        const failure = batchFailure("WATCH_CONTROL_FAILED", error);
        throw new EditBatchRequestError(
          "WATCH_CONTROL_FAILED",
          failure.message,
          failure,
        );
      }
      if (closing)
        throw new EditBatchRequestError(
          "SERVER_CLOSING",
          "Dev server closed while beginning the batch",
        );
      watchState = "held";
      batch.ready = true;
      if (policy.timeout.action !== "none") {
        batch.timer = setTimeout(() => {
          void expire(batch);
        }, policy.timeout.afterMs).unref();
      }
      return batch.id;
    },

    commit(id: string) {
      const batch = lookup(id);
      if (batch.result || batch.phase !== "holding") return batch.completion;
      checkOpen();
      if (!batch.ready)
        throw new EditBatchRequestError(
          "BATCH_BUSY",
          "The batch hold has not been acknowledged",
        );
      batch.phase = "committing";
      clearTimeout(batch.timer);
      // The async build function calls backend.build before its first suspension.
      void build(batch);
      return batch.completion;
    },

    wait(
      id: string,
      options: EditBatchWaitOptions = {},
    ): Promise<EditBatchResult> {
      const batch = lookup(id);
      if (options.timeoutMs !== undefined && !validTimeout(options.timeoutMs)) {
        throw new EditBatchRequestError(
          "INVALID_WAIT_OPTIONS",
          "timeoutMs must be a positive timer-safe integer",
        );
      }
      if (options.signal?.aborted) return Promise.reject(options.signal.reason);
      if (batch.result) return Promise.resolve(batch.result);
      return new Promise((resolve, reject) => {
        let timer: NodeJS.Timeout | undefined;
        const cleanup = () => {
          clearTimeout(timer);
          options.signal?.removeEventListener("abort", abort);
          batch.waiters.delete(done);
        };
        const done = (result: EditBatchResult) => {
          cleanup();
          resolve(result);
        };
        const abort = () => {
          cleanup();
          reject(options.signal?.reason);
        };
        batch.waiters.add(done);
        options.signal?.addEventListener("abort", abort, { once: true });
        if (options.timeoutMs !== undefined) {
          timer = setTimeout(() => {
            cleanup();
            reject(new EditBatchWaitTimeoutError(id));
          }, options.timeoutMs).unref();
        }
      });
    },

    activeId() {
      return active?.ready ? active.id : undefined;
    },

    close() {
      if (closePromise) return closePromise;
      // This synchronous decision beats any completion not already stored.
      closing = true;
      const batch = active;
      clearTimeout(batch?.timer);
      closePromise = Promise.resolve()
        .then(() => backend.close())
        .then(
          () => {
            watchState = "closed";
            if (batch && !batch.result) {
              batch.errors.push(
                batchFailure(
                  "SERVER_CLOSED",
                  new Error("Dev server closed before the edit batch settled"),
                ),
              );
              fail(batch, "close");
            }
          },
          (error: unknown) => {
            watchState = "unknown";
            if (batch && !batch.result) {
              batch.errors.push(batchFailure("CLOSE_FAILED", error));
              fail(batch, "close");
            }
            throw error;
          },
        );
      return closePromise;
    },
  };
}

export type EditBatchController = ReturnType<typeof createEditBatchController>;
