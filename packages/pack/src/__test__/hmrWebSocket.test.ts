import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static readonly instances: FakeWebSocket[] = [];

  readonly CONNECTING = FakeWebSocket.CONNECTING;
  readonly OPEN = FakeWebSocket.OPEN;
  readonly CLOSING = FakeWebSocket.CLOSING;
  readonly CLOSED = FakeWebSocket.CLOSED;
  readonly sent: string[] = [];
  readonly url: string;
  closeCalls = 0;
  readyState = FakeWebSocket.CONNECTING;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent<string>) => void) | null = null;

  constructor(url: string | URL) {
    this.url = String(url);
    FakeWebSocket.instances.push(this);
  }

  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.({ target: this } as unknown as Event);
  }

  fail() {
    this.onerror?.({ target: this } as unknown as Event);
  }

  send(message: string) {
    this.sent.push(message);
  }

  close() {
    this.closeCalls += 1;
    this.readyState = FakeWebSocket.CLOSED;
  }
}

function createWindow() {
  const listeners = new Map<string, Set<(event: any) => void>>();
  const location = {
    hostname: "localhost",
    port: "3000",
    protocol: "http:",
    reload: vi.fn(),
  };

  return {
    location,
    window: {
      console: {
        log: vi.fn(),
        warn: vi.fn(),
      },
      location,
      addEventListener(type: string, callback: (event: any) => void) {
        let callbacks = listeners.get(type);
        if (!callbacks) {
          callbacks = new Set();
          listeners.set(type, callbacks);
        }
        callbacks.add(callback);
      },
    },
    dispatch(type: string, event: any = {}) {
      for (const callback of listeners.get(type) ?? []) {
        callback(event);
      }
    },
  };
}

describe("HMR WebSocket", () => {
  const originalSocketServer = process.env.SOCKET_SERVER;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
    FakeWebSocket.instances.length = 0;
    delete process.env.SOCKET_SERVER;
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    if (originalSocketServer === undefined) {
      delete process.env.SOCKET_SERVER;
    } else {
      process.env.SOCKET_SERVER = originalSocketServer;
    }
  });

  it("reconnects and emits connected so active subscriptions can be restored", async () => {
    const browser = createWindow();
    vi.stubGlobal("window", browser.window);
    vi.stubGlobal("location", browser.location);
    vi.stubGlobal("WebSocket", FakeWebSocket);

    const { addMessageListener, connectHMR } = await import(
      "../../../../crates/pack-core/js/src/hmr/websocket"
    );
    const messages: Array<{ type: string }> = [];
    addMessageListener((message) => messages.push(message));

    connectHMR({ path: "/turbopack-hmr" });
    expect(FakeWebSocket.instances).toHaveLength(1);

    FakeWebSocket.instances[0].fail();
    await vi.advanceTimersByTimeAsync(499);
    expect(FakeWebSocket.instances).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(FakeWebSocket.instances).toHaveLength(2);
    FakeWebSocket.instances[1].open();

    expect(messages).toEqual([{ type: "turbopack-connected" }]);
  });

  it("cancels reconnects while the page is unloading", async () => {
    const browser = createWindow();
    vi.stubGlobal("window", browser.window);
    vi.stubGlobal("location", browser.location);
    vi.stubGlobal("WebSocket", FakeWebSocket);

    const { connectHMR } = await import(
      "../../../../crates/pack-core/js/src/hmr/websocket"
    );

    connectHMR({ path: "/turbopack-hmr" });
    FakeWebSocket.instances[0].fail();
    browser.dispatch("pagehide");
    await vi.advanceTimersByTimeAsync(10_000);

    expect(FakeWebSocket.instances).toHaveLength(1);
  });
});
