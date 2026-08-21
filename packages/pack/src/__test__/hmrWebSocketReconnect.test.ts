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

async function connectHmr(reconnect?: boolean | number) {
  const { connectHMR } = await import(
    "../../../../crates/pack-core/js/src/hmr/websocket"
  );
  connectHMR({ path: "/turbopack-hmr", reconnect });
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

  it("does not reconnect by default", async () => {
    const socket = await connectHmr();

    socket.onerror?.({ target: socket } as unknown as Event);
    await vi.advanceTimersByTimeAsync(60_000);

    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it("reconnects only the configured number of times", async () => {
    const socket = await connectHmr(2);
    socket.onerror?.({ target: socket } as unknown as Event);

    const retryDelays = [1_000, 2_000];
    for (const delay of retryDelays) {
      await vi.advanceTimersByTimeAsync(delay);
      const current = FakeWebSocket.instances.at(-1)!;
      current.onerror?.({ target: current } as unknown as Event);
    }

    await vi.advanceTimersByTimeAsync(60_000);

    expect(FakeWebSocket.instances).toHaveLength(3);
    expect(FakeWebSocket.instances[1].url).toBe(
      "ws://localhost:3000/turbopack-hmr",
    );
  });

  it("keeps reconnecting when explicitly enabled", async () => {
    const socket = await connectHmr(true);
    socket.onerror?.({ target: socket } as unknown as Event);

    const retryDelays = [1_000, 2_000, 5_000, 10_000, 30_000, 30_000];
    for (const delay of retryDelays) {
      await vi.advanceTimersByTimeAsync(delay);
      const current = FakeWebSocket.instances.at(-1)!;
      current.onerror?.({ target: current } as unknown as Event);
    }

    expect(FakeWebSocket.instances).toHaveLength(7);
  });
});
