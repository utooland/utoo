import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  serializeValue,
  shouldForward,
} from "../../../../crates/pack-core/js/src/hmr/browser-logs";
import { forwardBrowserLogs, isBrowserLogsMessage } from "../core/browserLogs";
import type { Project } from "../core/types";

const CONSOLE_METHODS = [
  "assert",
  "log",
  "info",
  "warn",
  "error",
  "debug",
  "table",
  "trace",
  "dir",
  "dirxml",
  "group",
  "groupCollapsed",
  "groupEnd",
] as const;

type ConsoleMethod = (typeof CONSOLE_METHODS)[number];
type BrowserToTerminal = boolean | "error" | "warn";
type ConnectedEvent = {
  type: string;
  browserToTerminal?: BrowserToTerminal;
};
type BrowserLogMessage = {
  event: "browser-logs";
  entries: Array<{
    method: ConsoleMethod;
    kind: "console" | "uncaught-error" | "unhandled-rejection";
    args: string[];
    stack?: string;
  }>;
};

async function createBrowserHarness(sendResults: boolean[] = [true]) {
  vi.resetModules();

  const consoleSpies = Object.fromEntries(
    CONSOLE_METHODS.map((method) => [method, vi.fn()]),
  ) as Record<ConsoleMethod, ReturnType<typeof vi.fn>>;
  const originalConsoleSpies = { ...consoleSpies };
  const eventListeners = new Map<string, Array<(event: unknown) => void>>();
  const animationFrames: FrameRequestCallback[] = [];
  let messageListener: ((event: ConnectedEvent) => void) | undefined;
  const sentMessages: BrowserLogMessage[] = [];

  vi.stubGlobal("console", consoleSpies);
  vi.stubGlobal("window", {
    addEventListener: vi.fn(
      (type: string, listener: (event: unknown) => void) => {
        const listeners = eventListeners.get(type) ?? [];
        listeners.push(listener);
        eventListeners.set(type, listeners);
      },
    ),
  });
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn((callback: FrameRequestCallback) => {
      animationFrames.push(callback);
      return animationFrames.length;
    }),
  );

  const sendMessage = vi.fn((message: BrowserLogMessage) => {
    sentMessages.push(message);
    return sendResults.shift() ?? true;
  });
  const { initializeBrowserLogForwarding } = await import(
    "../../../../crates/pack-core/js/src/hmr/browser-logs"
  );
  initializeBrowserLogForwarding({
    addMessageListener(listener) {
      messageListener = listener;
    },
    sendMessage,
  });

  return {
    consoleSpies,
    originalConsoleSpies,
    sentMessages,
    sendMessage,
    connect(browserToTerminal: BrowserToTerminal) {
      if (!messageListener) throw new Error("message listener was not added");
      messageListener({ type: "turbopack-connected", browserToTerminal });
    },
    dispatch(type: string, event: unknown) {
      for (const listener of eventListeners.get(type) ?? []) listener(event);
    },
    flushAnimationFrame() {
      const callback = animationFrames.shift();
      if (!callback) throw new Error("animation frame was not scheduled");
      callback(0);
    },
    get pendingAnimationFrames() {
      return animationFrames.length;
    },
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("browser log forwarding", () => {
  it.each([
    ["error", ["assert", "error"]],
    ["warn", ["assert", "error", "warn"]],
    [true, CONSOLE_METHODS],
    [false, []],
  ] as const)(
    "applies the %s forwarding threshold to every method",
    (level, expected) => {
      for (const method of CONSOLE_METHODS) {
        expect(shouldForward(level, method), method).toBe(
          expected.includes(method as never),
        );
      }
    },
  );

  it("serializes primitive and built-in browser values", () => {
    function namedFunction() {}

    expect(serializeValue("text")).toBe("text");
    expect(serializeValue(undefined)).toBe("undefined");
    expect(serializeValue(null)).toBe("null");
    expect(serializeValue(42)).toBe("42");
    expect(serializeValue(true)).toBe("true");
    expect(serializeValue(42n)).toBe("42n");
    expect(serializeValue(Symbol("token"))).toBe("Symbol(token)");
    expect(serializeValue(namedFunction)).toBe("[Function namedFunction]");
    expect(serializeValue(() => undefined)).toBe("[Function anonymous]");
    expect(serializeValue(new Error("broken"))).toBe("Error: broken");
    expect(serializeValue(new Date("2026-08-20T00:00:00.000Z"))).toBe(
      "2026-08-20T00:00:00.000Z",
    );
    expect(serializeValue(/utoo/gi)).toBe("/utoo/gi");
  });

  it("bounds strings, collection breadth, and nesting depth", () => {
    const longString = "x".repeat(10_001);
    const longArray = Array.from({ length: 101 }, (_, index) => index);
    const wideObject = Object.fromEntries(
      Array.from({ length: 101 }, (_, index) => [`key${index}`, index]),
    );
    const nestedObject = {
      one: { two: { three: { four: { five: { value: "hidden" } } } } },
    };
    const nestedArray = [[[[[["hidden"]]]]]];

    expect(serializeValue(longString)).toBe(`${"x".repeat(10_000)}…`);
    expect(serializeValue(longArray)).toContain("99, … 1 more]");
    expect(serializeValue(wideObject)).toContain("key99: 99, … 1 more }");
    expect(serializeValue(nestedObject)).toBe(
      "{ one: { two: { three: { four: { five: [Object] } } } } }",
    );
    expect(serializeValue(nestedArray)).toBe("[[[[[[Array]]]]]]");
  });

  it("serializes cycles and accessors without invoking user code", () => {
    const value: Record<string, unknown> = { count: 1, missing: undefined };
    value.self = value;
    Object.defineProperty(value, "secret", {
      enumerable: true,
      get() {
        throw new Error("getter must not run");
      },
    });

    expect(serializeValue(value)).toBe(
      "{ count: 1, missing: undefined, self: [Circular], secret: [Getter] }",
    );
    expect(
      serializeValue(
        new Proxy(
          {},
          {
            ownKeys() {
              throw new Error("cannot inspect");
            },
          },
        ),
      ),
    ).toBe("[Unserializable]");
  });

  it("forwards every console method in one animation-frame batch", async () => {
    const harness = await createBrowserHarness();
    harness.connect(true);

    for (const method of CONSOLE_METHODS) {
      if (method === "assert") harness.consoleSpies.assert(false, "assertion");
      else harness.consoleSpies[method](method);
    }
    harness.consoleSpies.assert(true, "must stay local");
    harness.consoleSpies.log("[HMR] internal update");

    expect(harness.pendingAnimationFrames).toBe(1);
    harness.flushAnimationFrame();

    expect(harness.sendMessage).toHaveBeenCalledTimes(1);
    expect(harness.sentMessages[0].entries.map(({ method }) => method)).toEqual(
      CONSOLE_METHODS,
    );
    expect(harness.sentMessages[0].entries[0].args).toEqual(["assertion"]);
    expect(harness.sentMessages[0].entries).not.toContainEqual(
      expect.objectContaining({ args: ["must stay local"] }),
    );
    expect(harness.sentMessages[0].entries).not.toContainEqual(
      expect.objectContaining({ args: ["[HMR] internal update"] }),
    );
    for (const method of CONSOLE_METHODS) {
      expect(harness.originalConsoleSpies[method], method).toHaveBeenCalled();
    }
  });

  it("queues logs before connection and caps the pending queue", async () => {
    const harness = await createBrowserHarness();

    for (let index = 0; index < 1_001; index++) {
      harness.consoleSpies.warn(index);
    }
    expect(harness.pendingAnimationFrames).toBe(0);

    harness.connect("warn");
    expect(harness.pendingAnimationFrames).toBe(1);
    harness.flushAnimationFrame();

    expect(harness.sentMessages[0].entries).toHaveLength(1_000);
    expect(harness.sentMessages[0].entries[0].args).toEqual(["1"]);
    expect(harness.sentMessages[0].entries.at(-1)?.args).toEqual(["1000"]);
  });

  it("retries a failed send after the HMR connection is re-established", async () => {
    const harness = await createBrowserHarness([false, true]);
    harness.connect("error");
    harness.consoleSpies.error("retry me");
    harness.flushAnimationFrame();

    expect(harness.sendMessage).toHaveBeenCalledTimes(1);
    expect(harness.sentMessages[0].entries[0].args).toEqual(["retry me"]);
    expect(harness.pendingAnimationFrames).toBe(0);

    harness.connect("error");
    expect(harness.pendingAnimationFrames).toBe(1);
    harness.flushAnimationFrame();

    expect(harness.sendMessage).toHaveBeenCalledTimes(2);
    expect(harness.sentMessages[1]).toEqual(harness.sentMessages[0]);
  });

  it("forwards window errors and unhandled rejections", async () => {
    const harness = await createBrowserHarness();
    harness.connect("error");
    const uncaught = new TypeError("uncaught failure");
    const rejection = new Error("async failure");

    harness.dispatch("error", { error: uncaught, message: uncaught.message });
    harness.dispatch("error", { error: null, message: "script failure" });
    harness.dispatch("unhandledrejection", { reason: rejection });
    harness.dispatch("unhandledrejection", { reason: { code: 503 } });

    expect(harness.pendingAnimationFrames).toBe(1);
    harness.flushAnimationFrame();

    expect(harness.sentMessages[0].entries).toEqual([
      expect.objectContaining({
        method: "error",
        kind: "uncaught-error",
        args: ["Uncaught TypeError: uncaught failure"],
        stack: uncaught.stack,
      }),
      expect.objectContaining({
        method: "error",
        kind: "uncaught-error",
        args: ["Uncaught Error: script failure"],
      }),
      expect.objectContaining({
        method: "error",
        kind: "unhandled-rejection",
        args: ["Unhandled Promise rejection: Error: async failure"],
        stack: rejection.stack,
      }),
      expect.objectContaining({
        method: "error",
        kind: "unhandled-rejection",
        args: ["Unhandled Promise rejection: { code: 503 }"],
      }),
    ]);
  });

  it("validates, filters, source maps, and prints browser messages", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const project = {
      traceSource: vi.fn(async () => ({
        isServer: false,
        file: "file:///project/src/app.ts",
        originalFile: "file:///project/src/app.ts",
        line: 7,
        column: 4,
      })),
    } as unknown as Project;
    const message = {
      event: "browser-logs" as const,
      entries: [
        {
          method: "log" as const,
          kind: "console" as const,
          args: ["ignored"],
        },
        {
          method: "warn" as const,
          kind: "console" as const,
          args: ["caution"],
          stack: "render@http://localhost:3000/main.js:10:2",
        },
      ],
    };

    expect(isBrowserLogsMessage(message)).toBe(true);
    await forwardBrowserLogs(
      message,
      "warn",
      project,
      "/project",
      "/project/dist",
    );

    expect(log).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledWith("[browser] caution (src/app.ts:7:4)");
    warn.mockRestore();
    log.mockRestore();
  });

  it("falls back to emitted source maps for browser asset URLs", async () => {
    const projectPath = fs.mkdtempSync(
      path.join(os.tmpdir(), "utoo-browser-logs-"),
    );
    const outputPath = path.join(projectPath, "dist");
    fs.mkdirSync(outputPath);
    fs.writeFileSync(
      path.join(outputPath, "bundle.js.map"),
      JSON.stringify({
        version: 3,
        sources: ["../src/input.js"],
        names: [],
        mappings: "AAAA",
      }),
    );
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const project = {
      traceSource: vi.fn(async () => null),
    } as unknown as Project;

    try {
      await forwardBrowserLogs(
        {
          event: "browser-logs",
          entries: [
            {
              method: "warn",
              kind: "console",
              args: ["mapped"],
              stack: "    at run (http://localhost:3000/bundle.js:1:1)",
            },
          ],
        },
        "warn",
        project,
        projectPath,
        outputPath,
      );

      expect(warn).toHaveBeenCalledWith("[browser] mapped (src/input.js:1:1)");
    } finally {
      warn.mockRestore();
      fs.rmSync(projectPath, { recursive: true, force: true });
    }
  });

  it("rejects malformed browser log payloads", () => {
    expect(
      isBrowserLogsMessage({
        event: "browser-logs",
        entries: [{ method: "fatal", kind: "console", args: [] }],
      }),
    ).toBe(false);
  });
});
