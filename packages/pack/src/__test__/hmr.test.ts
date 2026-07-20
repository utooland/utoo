import { describe, expect, it, vi } from "vitest";
import {
  consumeHmrSubscription,
  createHmrTimingReporter,
  createSharedHmrSubscriptionRegistry,
  deleteClientSubscriptionIfCurrent,
  enqueueTurbopackUpdateForClient,
  isCurrentClientSubscription,
  unsubscribeAllClientSubscriptions,
  unsubscribeClient,
} from "../core/hmrClientState";

describe("HMR timing reporter", () => {
  it("does not attribute the initial project batch to the first HMR update", () => {
    let now = 0;
    const reporter = createHmrTimingReporter(() => now);

    reporter.start();
    now = 60_000;

    expect(reporter.markHmrUpdate()).toBeUndefined();
  });

  it("reports time to the first HMR payload after the initial batch", () => {
    let now = 0;
    const reporter = createHmrTimingReporter(() => now);

    reporter.start();
    reporter.end();
    now = 100;
    reporter.start();
    now = 425;

    expect(reporter.markHmrUpdate()).toBe(325);
    expect(reporter.markHmrUpdate()).toBeUndefined();
  });

  it("does not report an update outside an active project batch", () => {
    const reporter = createHmrTimingReporter(() => 100);

    expect(reporter.markHmrUpdate()).toBeUndefined();
    reporter.start();
    reporter.end();
    expect(reporter.markHmrUpdate()).toBeUndefined();
  });

  it("keeps timing a payload that is sent just after its batch ends", () => {
    let now = 0;
    const reporter = createHmrTimingReporter(() => now);

    reporter.start();
    reporter.end();
    now = 100;
    reporter.start();
    now = 425;
    reporter.end();
    now = 450;

    expect(reporter.markHmrUpdate()).toBe(350);
  });

  it("uses the latest batch when a newer one starts before a payload", () => {
    let now = 0;
    const reporter = createHmrTimingReporter(() => now);

    reporter.start();
    reporter.end();
    now = 100;
    reporter.start();
    reporter.end();
    now = 300;
    reporter.start();
    now = 350;

    expect(reporter.markHmrUpdate()).toBe(50);
  });
});

async function* hmrEvents(...events: any[]) {
  yield* events;
}

function createControlledHmrEvents<Result>() {
  let settled = false;
  let pending:
    | {
        resolve: (result: IteratorResult<Result>) => void;
        reject: (error: unknown) => void;
      }
    | undefined;

  const iterator: AsyncIterableIterator<Result> & {
    emit(result: Result): void;
    fail(error: unknown): void;
  } = {
    next: vi.fn(() => {
      if (settled) return Promise.resolve({ done: true, value: undefined });
      return new Promise((resolve, reject) => {
        pending = { resolve, reject };
      });
    }),
    return: vi.fn(() => {
      settled = true;
      pending?.resolve({ done: true, value: undefined });
      pending = undefined;
      return Promise.resolve({ done: true, value: undefined });
    }),
    emit(result) {
      const current = pending;
      pending = undefined;
      current?.resolve({ done: false, value: result });
    },
    fail(error) {
      const current = pending;
      pending = undefined;
      current?.reject(error);
    },
    [Symbol.asyncIterator]() {
      return this;
    },
  };

  return iterator;
}

function createClientState() {
  return {
    hmrPayloads: new Map(),
    turbopackUpdates: [],
    subscriptions: new Map(),
    clientIssues: new Map(),
  };
}

describe("HMR client delivery", () => {
  it("queues a subscribed update only for the client that produced it", () => {
    const clientA = { send: vi.fn(), close: vi.fn() };
    const clientB = { send: vi.fn(), close: vi.fn() };
    const stateA = createClientState();
    const stateB = createClientState();
    const clientStates = new WeakMap([
      [clientA, stateA],
      [clientB, stateB],
    ]);
    const update = {
      type: "partial",
      issues: [{ severity: "warning" }],
    } as any;

    enqueueTurbopackUpdateForClient(clientStates, clientA, update);

    expect(stateA.turbopackUpdates).toEqual([{ ...update, issues: [] }]);
    expect(stateB.turbopackUpdates).toEqual([]);
    expect(update.issues).toEqual([{ severity: "warning" }]);
  });

  it("preserves distinct resources even when their instructions are equal", () => {
    const client = { send: vi.fn(), close: vi.fn() };
    const state = createClientState();
    const clientStates = new WeakMap([[client, state]]);
    const instruction = { type: "ChunkListUpdate", merged: [] };

    enqueueTurbopackUpdateForClient(clientStates, client, {
      type: "partial",
      resource: { path: "route-a" },
      instruction,
      issues: [],
    });
    enqueueTurbopackUpdateForClient(clientStates, client, {
      type: "partial",
      resource: { path: "route-b" },
      instruction,
      issues: [],
    });

    expect(
      state.turbopackUpdates.map((update: any) => update.resource.path),
    ).toEqual(["route-a", "route-b"]);
  });
});

describe("HMR subscription lifecycle", () => {
  it("shares one underlying iterator for subscribers to the same id", async () => {
    const baseline = { type: "issues", issues: [] };
    const update = { type: "partial", issues: [], instruction: {} };
    const createSubscription = vi.fn(() => hmrEvents(baseline, update));
    const registry = createSharedHmrSubscriptionRegistry(createSubscription);
    const clientA = { onIssues: vi.fn(), onUpdate: vi.fn() };
    const clientB = { onIssues: vi.fn(), onUpdate: vi.fn() };

    registry.subscribe("route", clientA);
    registry.subscribe("route", clientB);
    await vi.waitFor(() => {
      expect(clientA.onUpdate).toHaveBeenCalledWith(update);
      expect(clientB.onUpdate).toHaveBeenCalledWith(update);
    });

    expect(createSubscription).toHaveBeenCalledExactlyOnceWith("route");
    expect(clientA.onIssues).toHaveBeenCalledWith([]);
    expect(clientB.onIssues).toHaveBeenCalledWith([]);
  });

  it("returns the shared iterator only after its last subscriber leaves", async () => {
    const iterator = createControlledHmrEvents<any>();
    const registry = createSharedHmrSubscriptionRegistry(() => iterator);
    const subscriptionA = registry.subscribe("route", {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
    });
    const subscriptionB = registry.subscribe("route", {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
    });

    await subscriptionA.return?.();
    expect(iterator.return).not.toHaveBeenCalled();

    await subscriptionB.return?.();
    expect(iterator.return).toHaveBeenCalledOnce();
  });

  it("seeds a joining subscriber with latest issues without replaying a partial", async () => {
    const iterator = createControlledHmrEvents<any>();
    const registry = createSharedHmrSubscriptionRegistry(() => iterator);
    const clientA = { onIssues: vi.fn(), onUpdate: vi.fn() };
    const clientB = { onIssues: vi.fn(), onUpdate: vi.fn() };

    registry.subscribe("route", clientA);
    iterator.emit({
      type: "partial",
      issues: [{ severity: "warning", message: "latest" }],
      instruction: {},
    });
    await vi.waitFor(() => expect(clientA.onUpdate).toHaveBeenCalledOnce());

    registry.subscribe("route", clientB);

    expect(clientB.onIssues).toHaveBeenCalledExactlyOnceWith([
      { severity: "warning", message: "latest" },
    ]);
    expect(clientB.onUpdate).not.toHaveBeenCalled();
  });

  it("isolates issue snapshots between subscribers", async () => {
    const iterator = createControlledHmrEvents<any>();
    const registry = createSharedHmrSubscriptionRegistry(() => iterator);
    const clientA = {
      onIssues: vi.fn((issues: any[]) => issues.push({ severity: "fatal" })),
      onUpdate: vi.fn(),
    };
    const clientB = { onIssues: vi.fn(), onUpdate: vi.fn() };

    registry.subscribe("route", clientA);
    registry.subscribe("route", clientB);
    iterator.emit({ type: "issues", issues: [{ severity: "warning" }] });
    await vi.waitFor(() => expect(clientB.onIssues).toHaveBeenCalledOnce());

    expect(clientB.onIssues).toHaveBeenCalledWith([{ severity: "warning" }]);
  });

  it("keeps one subscriber callback failure from stopping other clients", async () => {
    const iterator = createControlledHmrEvents<any>();
    const registry = createSharedHmrSubscriptionRegistry(() => iterator);
    const clientA = {
      onIssues: vi.fn(() => {
        throw new Error("client state failed");
      }),
      onUpdate: vi.fn(),
      onError: vi.fn(),
    };
    const clientB = { onIssues: vi.fn(), onUpdate: vi.fn(), onError: vi.fn() };
    const update = { type: "partial", issues: [], instruction: {} };

    registry.subscribe("route", clientA);
    registry.subscribe("route", clientB);
    iterator.emit(update);
    await vi.waitFor(() => expect(clientB.onUpdate).toHaveBeenCalledOnce());

    expect(clientA.onError).toHaveBeenCalledOnce();
    expect(clientB.onError).not.toHaveBeenCalled();
    expect(clientB.onUpdate).toHaveBeenCalledWith(update);
  });

  it("does not let an old iterator error tear down its replacement", async () => {
    const oldIterator = createControlledHmrEvents<any>();
    oldIterator.return = vi.fn(() =>
      Promise.resolve({ done: true, value: undefined }),
    );
    const replacementIterator = createControlledHmrEvents<any>();
    const createSubscription = vi
      .fn()
      .mockReturnValueOnce(oldIterator)
      .mockReturnValueOnce(replacementIterator);
    const registry = createSharedHmrSubscriptionRegistry(createSubscription);
    const oldClient = { onIssues: vi.fn(), onUpdate: vi.fn() };
    const replacementClient = {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
      onError: vi.fn(),
    };

    const oldSubscription = registry.subscribe("route", oldClient);
    await oldSubscription.return?.();
    registry.subscribe("route", replacementClient);
    oldIterator.fail(new Error("stale iterator failed"));
    replacementIterator.emit({
      type: "partial",
      issues: [],
      instruction: { type: "replacement" },
    });
    await vi.waitFor(() =>
      expect(replacementClient.onUpdate).toHaveBeenCalledOnce(),
    );

    expect(createSubscription).toHaveBeenCalledTimes(2);
    expect(replacementClient.onError).not.toHaveBeenCalled();
  });

  it("reports a shared iterator error to every current subscriber", async () => {
    const iterator = createControlledHmrEvents<any>();
    const registry = createSharedHmrSubscriptionRegistry(() => iterator);
    const clientA = {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
      onError: vi.fn(),
      onComplete: vi.fn(),
    };
    const clientB = {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
      onError: vi.fn(),
      onComplete: vi.fn(),
    };
    const error = new Error("subscription failed");

    registry.subscribe("route", clientA);
    registry.subscribe("route", clientB);
    iterator.fail(error);
    await vi.waitFor(() => {
      expect(clientA.onComplete).toHaveBeenCalledOnce();
      expect(clientB.onComplete).toHaveBeenCalledOnce();
    });

    expect(clientA.onError).toHaveBeenCalledExactlyOnceWith(error);
    expect(clientB.onError).toHaveBeenCalledExactlyOnceWith(error);
  });

  it("absorbs shared iterator cleanup failures and permits replacement", async () => {
    const failingIterator = createControlledHmrEvents<any>();
    const finish = failingIterator.return!.bind(failingIterator);
    failingIterator.return = vi.fn(async () => {
      await finish();
      throw new Error("cleanup failed");
    });
    const replacementIterator = createControlledHmrEvents<any>();
    const createSubscription = vi
      .fn()
      .mockReturnValueOnce(failingIterator)
      .mockReturnValueOnce(replacementIterator);
    const registry = createSharedHmrSubscriptionRegistry(createSubscription);
    const subscription = registry.subscribe("route", {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
    });

    await expect(subscription.return?.()).resolves.toBeUndefined();
    registry.subscribe("route", {
      onIssues: vi.fn(),
      onUpdate: vi.fn(),
    });

    expect(createSubscription).toHaveBeenCalledTimes(2);
  });

  it("processes an initial issues baseline without forwarding an update", async () => {
    const onResult = vi.fn();
    const onUpdate = vi.fn();

    await consumeHmrSubscription(
      hmrEvents({ type: "issues", issues: [] }),
      onResult,
      onUpdate,
    );

    expect(onResult).toHaveBeenCalledOnce();
    expect(onUpdate).not.toHaveBeenCalled();
  });

  it("forwards a real partial update in the first subscription result", async () => {
    const update = { type: "partial", issues: [], instruction: {} };
    const onResult = vi.fn();
    const onUpdate = vi.fn();

    await consumeHmrSubscription(hmrEvents(update), onResult, onUpdate);

    expect(onResult).toHaveBeenCalledWith(update);
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith(update);
  });

  it("does not drop a buffered update immediately after the baseline", async () => {
    const baseline = { type: "issues", issues: [] };
    const restart = { type: "restart", issues: [] };
    const onResult = vi.fn();
    const onUpdate = vi.fn();

    await consumeHmrSubscription(
      hmrEvents(baseline, restart),
      onResult,
      onUpdate,
    );

    expect(onResult.mock.calls).toEqual([[baseline], [restart]]);
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith(restart);
  });

  it("safely cleans every subscription when a client disconnects", async () => {
    const remainingSizes: number[] = [];
    const subscriptions = new Map([
      [
        "throwing",
        {
          return: vi.fn(() => {
            remainingSizes.push(subscriptions.size);
            throw new Error("cleanup failed");
          }),
        },
      ],
      [
        "rejecting",
        {
          return: vi.fn(() => {
            remainingSizes.push(subscriptions.size);
            return Promise.reject(new Error("async cleanup failed"));
          }),
        },
      ],
    ]);

    const cleanup = unsubscribeAllClientSubscriptions(subscriptions);

    expect(subscriptions.size).toBe(0);
    expect(remainingSizes).toEqual([0, 0]);
    await expect(cleanup).resolves.toBeUndefined();
  });

  it("deletes a subscription before returning it so it can resubscribe", () => {
    const subscription = { return: vi.fn() };
    const subscriptions = new Map([["route", subscription]]);

    unsubscribeClient(subscriptions, "route");

    expect(subscriptions.has("route")).toBe(false);
    expect(subscription.return).toHaveBeenCalledOnce();
  });

  it("absorbs iterator cleanup failures after deleting the subscription", async () => {
    const subscription = {
      return: vi.fn(() => Promise.reject(new Error("cleanup failed"))),
    };
    const subscriptions = new Map([["route", subscription]]);

    await expect(
      unsubscribeClient(subscriptions, "route"),
    ).resolves.toBeUndefined();

    expect(subscriptions.has("route")).toBe(false);
    expect(subscription.return).toHaveBeenCalledOnce();
  });

  it("does not let an old iterator completion delete its replacement", () => {
    const oldSubscription = { return: vi.fn() };
    const newSubscription = { return: vi.fn() };
    const subscriptions = new Map([["route", newSubscription]]);

    deleteClientSubscriptionIfCurrent(subscriptions, "route", oldSubscription);

    expect(subscriptions.get("route")).toBe(newSubscription);
  });

  it("does not treat an old iterator error as current after resubscribing", () => {
    const client = { send: vi.fn(), close: vi.fn() };
    const state = createClientState();
    const clientStates = new WeakMap([[client, state]]);
    const oldSubscription = { return: vi.fn() };
    const newSubscription = { return: vi.fn() };

    state.subscriptions.set("route", oldSubscription);
    unsubscribeClient(state.subscriptions, "route");
    state.subscriptions.set("route", newSubscription);

    expect(
      isCurrentClientSubscription(
        clientStates,
        client,
        state,
        "route",
        oldSubscription,
      ),
    ).toBe(false);
    expect(
      isCurrentClientSubscription(
        clientStates,
        client,
        state,
        "route",
        newSubscription,
      ),
    ).toBe(true);
  });

  it("reports whether completion removed the current subscription", () => {
    const subscription = { return: vi.fn() };
    const subscriptions = new Map([["route", subscription]]);

    expect(
      deleteClientSubscriptionIfCurrent(subscriptions, "route", subscription),
    ).toBe(true);
    expect(
      deleteClientSubscriptionIfCurrent(subscriptions, "route", subscription),
    ).toBe(false);
  });
});
