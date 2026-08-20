import fs from "fs";
import os from "os";
import path from "path";
import { describe, expect, it, vi } from "vitest";
import {
  serializeValue,
  shouldForward,
} from "../../../../crates/pack-core/js/src/hmr/browser-logs";
import { forwardBrowserLogs, isBrowserLogsMessage } from "../core/browserLogs";
import type { Project } from "../core/types";

describe("browser log forwarding", () => {
  it("applies the configured forwarding threshold", () => {
    expect(shouldForward("error", "error")).toBe(true);
    expect(shouldForward("error", "warn")).toBe(false);
    expect(shouldForward("warn", "warn")).toBe(true);
    expect(shouldForward("warn", "info")).toBe(false);
    expect(shouldForward("error", "assert")).toBe(true);
    expect(shouldForward(true, "debug")).toBe(true);
    expect(shouldForward(false, "error")).toBe(false);
  });

  it("serializes browser values without invoking accessors or failing on cycles", () => {
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
    expect(serializeValue(42n)).toBe("42n");
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
