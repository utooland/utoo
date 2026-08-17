import { describe, expect, it, vi } from "vitest";
import { consumeHmrSubscription } from "../core/hmrSubscription";

async function* hmrEvents(...events: Array<{ type: string }>) {
  yield* events;
}

describe("HMR subscription", () => {
  it("forwards a restart returned by the initial version check", async () => {
    const restart = { type: "restart" };
    const onResult = vi.fn();
    const onUpdate = vi.fn();

    await consumeHmrSubscription(hmrEvents(restart), onResult, onUpdate);

    expect(onResult).toHaveBeenCalledExactlyOnceWith(restart);
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith(restart);
  });

  it("keeps the initial issues baseline out of the update stream", async () => {
    const baseline = { type: "issues" };
    const update = { type: "partial" };
    const onResult = vi.fn();
    const onUpdate = vi.fn();

    await consumeHmrSubscription(
      hmrEvents(baseline, update),
      onResult,
      onUpdate,
    );

    expect(onResult).toHaveBeenNthCalledWith(1, baseline);
    expect(onResult).toHaveBeenNthCalledWith(2, update);
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith(update);
  });
});
