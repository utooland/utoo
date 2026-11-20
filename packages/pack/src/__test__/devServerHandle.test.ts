import { EventEmitter } from "events";
import fs from "fs";
import http from "http";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";

const serverMocks = vi.hoisted(() => ({
  createHotReloader: vi.fn(),
  servers: [] as import("http").Server[],
}));

vi.mock("../core/hmr", () => ({
  createHotReloader: serverMocks.createHotReloader,
}));
vi.mock("@hono/node-server", async (importOriginal) => {
  const original = await importOriginal<typeof import("@hono/node-server")>();
  return {
    ...original,
    serve: (...args: Parameters<typeof original.serve>) => {
      const server = original.serve(...args);
      serverMocks.servers.push(server as http.Server);
      return server;
    },
  };
});

import { type DevServerHandle, serve } from "../commands/dev";

const listenerEvents = [
  "SIGINT",
  "SIGTERM",
  "rejectionHandled",
  "uncaughtException",
  "unhandledRejection",
] as const;
const handles: DevServerHandle[] = [];
let projectPath: string;
let oldTitle: string;
let oldNodeEnv: string | undefined;
let originalListeners: Map<string, ReturnType<typeof getProcessListeners>>;

function getProcessListeners(event: string) {
  return EventEmitter.prototype.listeners.call(process, event) as Array<
    Parameters<typeof EventEmitter.prototype.removeListener>[1]
  >;
}

async function closeHttpServer(server: http.Server): Promise<void> {
  if (!server.listening) return;
  await new Promise<void>((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
    if (
      "closeAllConnections" in server &&
      typeof server.closeAllConnections === "function"
    ) {
      server.closeAllConnections();
    }
  });
}

async function expectPortCanBeReused(port: number): Promise<void> {
  const server = http.createServer();
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(port, "127.0.0.1", resolve);
    });
  } finally {
    await closeHttpServer(server);
  }
}

async function getAvailablePort(): Promise<number> {
  const server = http.createServer();
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    const address = server.address();
    if (!address || typeof address === "string") {
      throw new Error("Expected a TCP listener address");
    }
    return address.port;
  } finally {
    await closeHttpServer(server);
  }
}

function createRuntime() {
  const sourcePath = path.join(projectPath, "dependency.js");
  const entryPath = path.join(projectPath, "entry.js");
  const outputPath = path.join(projectPath, "dist/main.js");
  fs.writeFileSync(sourcePath, "A");
  fs.writeFileSync(entryPath, 'import heading from "./dependency.js";\n');
  const project = {
    writeAllEntrypointsToDisk: vi.fn(async function (this: unknown) {
      expect(this).toBe(project);
      const heading = fs.readFileSync(sourcePath, "utf8");
      fs.mkdirSync(path.dirname(outputPath), { recursive: true });
      fs.writeFileSync(
        outputPath,
        `window.heading = ${JSON.stringify(heading)};`,
      );
      return {
        apps: [],
        libraries: [],
        appPaths: [
          { type: "app", entryPath: outputPath, clientPaths: [outputPath] },
        ],
        libraryPaths: [],
        issues: [],
        diagnostics: [],
      };
    }),
  };
  const runtime = {
    turbopackProject: project,
    writeAllEntrypointsToDisk: vi.fn(() => project.writeAllEntrypointsToDisk()),
    start: vi.fn(async () => {
      await project.writeAllEntrypointsToDisk();
    }),
    close: vi.fn(async () => {}),
    getClientPaths: vi.fn(() => [outputPath]),
    registerClient: vi.fn(),
    unregisterClient: vi.fn(),
    handleClientMessage: vi.fn(),
  };
  serverMocks.createHotReloader.mockResolvedValue(runtime);
  return { runtime, project, sourcePath, entryPath, outputPath };
}

async function startServer(
  handleSignals?: boolean,
  onReady?: () => void | Promise<void>,
) {
  const handle = await serve(
    {
      config: {
        entry: [{ name: "main", import: "./entry.js" }],
        output: { path: "./dist" },
      },
    },
    projectPath,
    projectPath,
    {
      port: await getAvailablePort(),
      hostname: "127.0.0.1",
      handleSignals,
      returnHandle: true,
      logServerInfo: false,
      onReady,
    },
  );
  handles.push(handle);
  return handle;
}

beforeEach(() => {
  projectPath = fs.mkdtempSync(
    path.join(os.tmpdir(), "utoo-dev-server-handle-"),
  );
  oldTitle = process.title;
  oldNodeEnv = process.env.NODE_ENV;
  originalListeners = new Map(
    listenerEvents.map((event) => [event, getProcessListeners(event)]),
  );
  serverMocks.createHotReloader.mockReset();
});

afterEach(async () => {
  await Promise.allSettled(handles.splice(0).map((handle) => handle?.close()));
  await Promise.allSettled(serverMocks.servers.splice(0).map(closeHttpServer));
  for (const event of listenerEvents) {
    const original = originalListeners.get(event) || [];
    for (const listener of getProcessListeners(event)) {
      if (!original.includes(listener)) process.removeListener(event, listener);
    }
  }
  fs.rmSync(projectPath, { recursive: true, force: true });
  process.title = oldTitle;
  if (oldNodeEnv === undefined) delete process.env.NODE_ENV;
  else process.env.NODE_ENV = oldNodeEnv;
  vi.restoreAllMocks();
});

describe("dev server handle", () => {
  it.each([
    ["webpack entry", undefined],
    ["webpack entry", true],
    ["workspace metadata", undefined],
    ["workspace metadata", true],
  ] as const)(
    "throws %s preparation errors synchronously (returnHandle: %s)",
    (source, returnHandle) => {
      if (source === "webpack entry") {
        fs.writeFileSync(
          path.join(projectPath, "webpack.config.cjs"),
          "module.exports = { entry: [] };\n",
        );
      } else {
        fs.writeFileSync(path.join(projectPath, "package.json"), "{ invalid");
      }
      const options =
        source === "webpack entry"
          ? { webpackMode: true as const }
          : { config: { entry: [{ name: "main", import: "./entry.js" }] } };

      expect(() => {
        const result = returnHandle
          ? serve(options, projectPath, undefined, {
              port: 0,
              returnHandle: true,
            })
          : serve(options, projectPath, undefined, { port: 0 });
        void result.catch(() => {});
      }).toThrow(source === "webpack entry" ? /missing "entry"/ : SyntaxError);
      expect(serverMocks.createHotReloader).not.toHaveBeenCalled();
    },
  );

  it.each([undefined, false] as const)(
    "preserves the legacy void result (returnHandle: %s)",
    async (returnHandle) => {
      const { runtime } = createRuntime();
      const result: Promise<void> = serve(
        {
          config: {
            entry: [{ name: "main", import: "./entry.js" }],
            output: { path: "./dist" },
          },
        },
        projectPath,
        projectPath,
        {
          port: await getAvailablePort(),
          hostname: "127.0.0.1",
          returnHandle,
          handleSignals: false,
          logServerInfo: false,
        },
      );

      expect(result).toBeInstanceOf(Promise);
      expect(await result).toBeUndefined();
      const server = serverMocks.servers[0];
      expect(server.listening).toBe(true);
      await closeHttpServer(server);
      await runtime.close();
    },
  );

  it("returns a usable handle and rewrites HTTP assets after a dependency changes", async () => {
    const { runtime, project, sourcePath, entryPath } = createRuntime();
    const originalEntry = fs.readFileSync(entryPath, "utf8");
    const entryMtime = fs.statSync(entryPath).mtimeMs;
    const handle = await startServer();

    expect(handle.port).toBeGreaterThan(0);
    expect(handle.hostname).toBe("127.0.0.1");
    expect(typeof handle.writeAllEntrypointsToDisk).toBe("function");
    expect(typeof handle.close).toBe("function");
    const url = `http://${handle.hostname}:${handle.port}/main.js`;
    expect(await (await fetch(url)).text()).toContain('"A"');
    fs.writeFileSync(sourcePath, "B");

    const written = await handle.writeAllEntrypointsToDisk();

    expect(written.issues).toEqual([]);
    expect(runtime.writeAllEntrypointsToDisk).toHaveBeenCalledTimes(1);
    expect(project.writeAllEntrypointsToDisk).toHaveBeenCalledTimes(2);
    expect(fs.readFileSync(entryPath, "utf8")).toBe(originalEntry);
    expect(fs.statSync(entryPath).mtimeMs).toBe(entryMtime);
    expect(await (await fetch(`${url}?after-edit=1`)).text()).toContain('"B"');
  });

  it.each(["before startup", "during the initial build"])(
    "uses an available port when the requested port is occupied %s",
    async (when) => {
      const { runtime } = createRuntime();
      const port = await getAvailablePort();
      const occupied = http.createServer();
      serverMocks.servers.push(occupied);
      const occupyPort = () =>
        new Promise<void>((resolve, reject) => {
          occupied.once("error", reject);
          occupied.listen(port, "127.0.0.1", resolve);
        });
      if (when === "before startup") await occupyPort();
      else {
        const build = runtime.start.getMockImplementation()!;
        runtime.start.mockImplementationOnce(async () => {
          await build();
          await occupyPort();
        });
      }
      const onReady = vi.fn(async (context: { port: number }) => {
        expect(context.port).not.toBe(port);
        const response = await fetch(
          `http://127.0.0.1:${context.port}/main.js`,
        );
        expect(response.status).toBe(200);
        expect(await response.text()).toContain('"A"');
      });

      const handle = await serve(
        { config: { entry: [{ name: "main", import: "./entry.js" }] } },
        projectPath,
        projectPath,
        {
          port,
          hostname: "127.0.0.1",
          returnHandle: true,
          handleSignals: false,
          logServerInfo: false,
          onReady,
        },
      );
      handles.push(handle);

      expect(onReady).toHaveBeenCalledTimes(1);
      expect(handle.port).toBe(onReady.mock.calls[0][0].port);
      const socket = new WebSocket(
        `ws://${handle.hostname}:${handle.port}/turbopack-hmr`,
      );
      await new Promise<void>((resolve, reject) => {
        socket.once("open", resolve);
        socket.once("error", reject);
      });
      await handle.close();

      expect(occupied.listening).toBe(true);
      expect(runtime.close).toHaveBeenCalledTimes(1);
      await expectPortCanBeReused(handle.port);
    },
  );

  it("closes idempotently, releases the HTTP port, and keeps the caller running", async () => {
    const { runtime } = createRuntime();
    const exit = vi.spyOn(process, "exit").mockImplementation(() => {
      throw new Error("Programmatic close must not exit the caller");
    });
    const handle = await startServer();
    const socket = new WebSocket(
      `ws://${handle.hostname}:${handle.port}/turbopack-hmr`,
    );
    await new Promise<void>((resolve, reject) => {
      socket.once("open", resolve);
      socket.once("error", reject);
    });
    await vi.waitFor(() =>
      expect(runtime.registerClient).toHaveBeenCalledTimes(1),
    );
    const socketClosed = new Promise<void>((resolve) =>
      socket.once("close", resolve),
    );

    const firstClose = handle.close();
    const secondClose = handle.close();
    expect(secondClose).toBe(firstClose);
    await Promise.all([firstClose, secondClose]);
    await handle.close();
    await socketClosed;

    expect(runtime.close).toHaveBeenCalledTimes(1);
    expect(socket.readyState).toBe(WebSocket.CLOSED);
    expect(exit).not.toHaveBeenCalled();
    await expectPortCanBeReused(handle.port);
  });

  it.each(["EADDRNOTAVAIL", "EADDRINUSE"])(
    "cleans up after a terminal %s listen error without retrying indefinitely",
    async (code) => {
      const { runtime } = createRuntime();
      const error = Object.assign(new Error("Cannot bind the HTTP listener"), {
        code,
      });
      const build = runtime.start.getMockImplementation()!;
      const listen = vi.fn(function (this: http.Server) {
        queueMicrotask(() => this.emit("error", error));
        return this;
      });
      runtime.start.mockImplementationOnce(async () => {
        await build();
        vi.spyOn(http.Server.prototype, "listen").mockImplementation(listen);
      });

      await expect(startServer()).rejects.toBe(error);

      expect(listen).toHaveBeenCalledTimes(code === "EADDRINUSE" ? 2 : 1);
      expect(runtime.close).toHaveBeenCalledTimes(1);
      expect(serverMocks.servers[0].listening).toBe(false);
      expect(serverMocks.servers[0].listenerCount("error")).toBe(0);
      for (const event of listenerEvents) {
        expect(getProcessListeners(event)).toEqual(
          originalListeners.get(event),
        );
      }
    },
  );

  it("still releases the HTTP listener when closing the runtime fails", async () => {
    const { runtime } = createRuntime();
    const error = new Error("runtime persistence failed during shutdown");
    runtime.close.mockRejectedValueOnce(error);
    const handle = await startServer();

    await expect(handle.close()).rejects.toMatchObject({ errors: [error] });

    expect(runtime.close).toHaveBeenCalledTimes(1);
    await expectPortCanBeReused(handle.port);
  });

  it.each([false, true, undefined])(
    "restores process listeners after close (handleSignals: %s)",
    async (handleSignals) => {
      createRuntime();
      const handle = await startServer(handleSignals);
      if (handleSignals === false) {
        for (const event of listenerEvents) {
          expect(getProcessListeners(event)).toEqual(
            originalListeners.get(event),
          );
        }
      } else {
        for (const event of listenerEvents) {
          expect(getProcessListeners(event).length).toBe(
            (originalListeners.get(event)?.length || 0) + 1,
          );
        }
      }

      await handle.close();

      for (const event of listenerEvents) {
        expect(getProcessListeners(event)).toEqual(
          originalListeners.get(event),
        );
      }
    },
  );

  it("releases the HTTP server and runtime when onReady rejects", async () => {
    const { runtime } = createRuntime();
    const error = new Error("embedding integration failed during startup");
    let listeningPort = 0;

    await expect(
      serve(
        {
          config: {
            entry: [{ name: "main", import: "./entry.js" }],
            output: { path: "./dist" },
          },
        },
        projectPath,
        projectPath,
        {
          port: await getAvailablePort(),
          hostname: "127.0.0.1",
          handleSignals: true,
          logServerInfo: false,
          onReady(context) {
            listeningPort = context.port;
            throw error;
          },
        },
      ),
    ).rejects.toBe(error);

    expect(runtime.close).toHaveBeenCalledTimes(1);
    expect(listeningPort).toBeGreaterThan(0);
    await expectPortCanBeReused(listeningPort);
    for (const event of listenerEvents) {
      expect(getProcessListeners(event)).toEqual(originalListeners.get(event));
    }
  });

  it("cleans up the runtime if its initial build fails", async () => {
    const { runtime } = createRuntime();
    const error = new Error("cannot finish the initial build");
    runtime.start.mockRejectedValueOnce(error);

    await expect(startServer()).rejects.toBe(error);

    expect(runtime.close).toHaveBeenCalledTimes(1);
    expect(serverMocks.servers).toHaveLength(0);
  });
});
