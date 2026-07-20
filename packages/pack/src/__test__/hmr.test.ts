import { describe, expect, it, vi } from "vitest";
import {
  deleteClientSubscriptionIfCurrent,
  enqueueTurbopackUpdateForClient,
  isCurrentClientSubscription,
  unsubscribeAllClientSubscriptions,
  unsubscribeClient,
} from "../core/hmrClientState";

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
