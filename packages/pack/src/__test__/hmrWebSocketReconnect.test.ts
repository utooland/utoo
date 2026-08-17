import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type SocketCallback = ((event?: Event) => void) | null;

class FakeWebSocket {
  static readonly instances: FakeWebSocket[] = [];

  readonly OPEN = 1;
  readyState = 0;
  onopen: SocketCallback = null;
  onerror: SocketCallback = null;
  onclose: SocketCallback = null;
  onmessage: ((event: MessageEvent<string>) => void) | null = null;

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }

  close() {}

  send() {}
}

async function connectHmr() {
  const { connectHMR } = await import(
    "../../../../crates/pack-core/js/src/hmr/websocket"
  );
  connectHMR({ path: "/turbopack-hmr" });
  return FakeWebSocket.instances[0];
}

describe("HMR WebSocket reconnect", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    FakeWebSocket.instances.length = 0;
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal("location", {
      hostname: "localhost",
      port: "3000",
      protocol: "http:",
    });
    vi.stubGlobal("window", {
      console: {
        error: vi.fn(),
        log: vi.fn(),
        warn: vi.fn(),
      },
      location: { reload: vi.fn() },
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("does not retry a socket that never connected", async () => {
    const socket = await connectHmr();

    socket.onerror?.({ target: socket } as unknown as Event);
    await vi.advanceTimersByTimeAsync(60_000);

    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it("reconnects after an established socket disconnects", async () => {
    const socket = await connectHmr();
    socket.readyState = socket.OPEN;
    socket.onopen?.();

    socket.onclose?.({ target: socket } as unknown as Event);
    await vi.advanceTimersByTimeAsync(999);
    expect(FakeWebSocket.instances).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(FakeWebSocket.instances[1].url).toBe(
      "ws://localhost:3000/turbopack-hmr",
    );
  });

  it("stops retrying after the bounded backoff is exhausted", async () => {
    const socket = await connectHmr();
    socket.readyState = socket.OPEN;
    socket.onopen?.();

    const retryDelays = [1_000, 2_000, 5_000, 10_000, 30_000];
    for (const delay of retryDelays) {
      const current = FakeWebSocket.instances.at(-1)!;
      current.onclose?.({ target: current } as unknown as Event);
      await vi.advanceTimersByTimeAsync(delay);
    }

    const finalAttempt = FakeWebSocket.instances.at(-1)!;
    finalAttempt.onerror?.({ target: finalAttempt } as unknown as Event);
    await vi.advanceTimersByTimeAsync(60_000);

    expect(FakeWebSocket.instances).toHaveLength(6);
  });
});
