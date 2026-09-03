import type {
  BrowserLogMethod,
  BrowserLogsMessage,
  BrowserToTerminal,
} from "@utoo/pack-shared";
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

const projectMocks = vi.hoisted(() => {
  const project = {
    entrypointsSubscribe: () =>
      (async function* () {
        yield { apps: [], issues: [], libraries: [] };
      })(),
    onExit: vi.fn(async () => {}),
    shutdown: vi.fn(async () => {}),
    traceSource: vi.fn(async () => null),
    updateInfoSubscribe: () => (async function* () {})(),
    getCompletedTaskCount: vi.fn(() => 0),
  };

  return {
    project,
    projectFactory: vi.fn(() => async () => project),
  };
});

vi.mock("../core/project", () => ({
  projectFactory: projectMocks.projectFactory,
}));

import { forwardBrowserLogs, isBrowserLogsMessage } from "../core/browserLogs";
import {
  createHotReloader,
  HMR_CLIENT_MESSAGE_MAX_BYTES,
  type HotReloaderInterface,
  parseHmrClientMessage,
  type WSLike,
} from "../core/hmr";
import type { Project } from "../core/types";

const METHODS = [
  "assert",
  "debug",
  "dir",
  "dirxml",
  "error",
  "group",
  "groupCollapsed",
  "groupEnd",
  "info",
  "log",
  "table",
  "trace",
  "warn",
] as const satisfies readonly BrowserLogMethod[];

const tempDirs: string[] = [];
let hotReloader: HotReloaderInterface | undefined;

function boundaryMessage(extraBytes = 0): string {
  const prefix = '{"event":"client-success","padding":"';
  const suffix = '"}';
  const available =
    HMR_CLIENT_MESSAGE_MAX_BYTES -
    Buffer.byteLength(prefix) -
    Buffer.byteLength(suffix) +
    extraBytes;
  return `${prefix}${"界".repeat(Math.floor(available / 3))}${"x".repeat(available % 3)}${suffix}`;
}

afterEach(async () => {
  await hotReloader?.close();
  hotReloader = undefined;
  for (const dir of tempDirs) {
    fs.rmSync(dir, { force: true, recursive: true });
  }
  tempDirs.length = 0;
  vi.restoreAllMocks();
});

describe("browser log server boundaries", () => {
  it("accepts every supported method and enforces payload limits", () => {
    const entries = METHODS.map((method) => ({
      args: [],
      kind: "console" as const,
      method,
    }));
    expect(isBrowserLogsMessage({ event: "browser-logs", entries })).toBe(true);
    expect(
      isBrowserLogsMessage({
        event: "browser-logs",
        entries: Array.from({ length: 1_000 }, () => entries[0]),
      }),
    ).toBe(true);
    expect(
      isBrowserLogsMessage({
        event: "browser-logs",
        entries: Array.from({ length: 1_001 }, () => entries[0]),
      }),
    ).toBe(false);

    const baseEntry = {
      args: Array.from({ length: 100 }, () => "argument"),
      kind: "uncaught-error" as const,
      method: "error" as const,
      stack: "Error: example",
    };
    expect(
      isBrowserLogsMessage({ event: "browser-logs", entries: [baseEntry] }),
    ).toBe(true);
    expect(
      isBrowserLogsMessage({
        event: "browser-logs",
        entries: [{ ...baseEntry, args: [...baseEntry.args, "overflow"] }],
      }),
    ).toBe(false);
    expect(
      isBrowserLogsMessage({
        event: "browser-logs",
        entries: [{ ...baseEntry, stack: null }],
      }),
    ).toBe(false);
  });

  it("defensively filters false, error, warn, and true on the server", async () => {
    const spies = new Map(
      METHODS.map((method) => [
        method,
        vi.spyOn(console, method).mockImplementation((() => {}) as never),
      ]),
    );
    const message: BrowserLogsMessage = {
      event: "browser-logs",
      entries: METHODS.map((method) => ({
        args: [method],
        kind: "console",
        method,
      })),
    };
    const project = projectMocks.project as unknown as Project;

    const callsFor = async (level: BrowserToTerminal) => {
      for (const spy of spies.values()) spy.mockClear();
      await forwardBrowserLogs(message, level, project, "/project", "/dist");
      return Object.fromEntries(
        METHODS.map((method) => [method, spies.get(method)?.mock.calls.length]),
      );
    };
    const calledMethods = (calls: Record<string, number | undefined>) =>
      Object.entries(calls)
        .filter(([, count]) => (count ?? 0) > 0)
        .map(([method]) => method);

    expect(calledMethods(await callsFor(false))).toEqual([]);
    expect(calledMethods(await callsFor("error"))).toEqual(["assert", "error"]);
    expect(calledMethods(await callsFor("warn"))).toEqual([
      "assert",
      "error",
      "warn",
    ]);
    expect(await callsFor(true)).toEqual({
      assert: 1,
      debug: 1,
      dir: 1,
      dirxml: 1,
      error: 1,
      group: 1,
      groupCollapsed: 1,
      groupEnd: 1,
      info: 1,
      log: 2,
      table: 1,
      trace: 0,
      warn: 1,
    });
  });
});

describe("HMR browser message boundaries", () => {
  it("counts UTF-8 bytes and accepts exactly 1 MB", () => {
    const exact = boundaryMessage();
    expect(Buffer.byteLength(exact)).toBe(HMR_CLIENT_MESSAGE_MAX_BYTES);
    expect(parseHmrClientMessage(exact).status).toBe("ok");

    const overflow = boundaryMessage(1);
    expect(Buffer.byteLength(overflow)).toBe(HMR_CLIENT_MESSAGE_MAX_BYTES + 1);
    expect(parseHmrClientMessage(overflow)).toEqual({ status: "too-large" });
  });

  it("normalizes non-string legacy WebSocket payloads", () => {
    const serialized = JSON.stringify({ event: "client-success" });
    const bytes = Buffer.from(serialized);
    const arrayBuffer = bytes.buffer.slice(
      bytes.byteOffset,
      bytes.byteOffset + bytes.byteLength,
    );

    for (const payload of [
      bytes,
      [bytes.subarray(0, 4), bytes.subarray(4)],
      arrayBuffer,
      new Uint8Array(arrayBuffer),
    ]) {
      expect(parseHmrClientMessage(payload)).toEqual({
        status: "ok",
        value: { event: "client-success" },
      });
    }
  });

  it("ignores malformed JSON and closes oversized current messages with 1009", async () => {
    vi.spyOn(console, "log").mockImplementation(() => {});
    const projectPath = fs.mkdtempSync(
      path.join(os.tmpdir(), "utoo-browser-log-hmr-"),
    );
    tempDirs.push(projectPath);
    hotReloader = await createHotReloader(
      {
        config: {
          entry: [],
          output: { clean: true, path: "./dist" },
        },
      } as never,
      projectPath,
      projectPath,
    );
    const client: WSLike = {
      close: vi.fn(),
      send: vi.fn(),
    };

    expect(() => hotReloader?.handleClientMessage(client, "{")).not.toThrow();
    expect(client.close).not.toHaveBeenCalled();

    hotReloader.handleClientMessage(client, boundaryMessage(1));
    expect(client.close).toHaveBeenCalledWith(
      1009,
      "HMR client message exceeds the 1 MB limit",
    );
  });
});
