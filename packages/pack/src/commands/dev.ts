import fs from "fs";
import http, { IncomingMessage, ServerResponse } from "http";
import https from "https";
import { isIPv6 } from "net";
import path from "path";
import send from "send";
import { Duplex, Writable } from "stream";
import url from "url";
import { resolveBundleOptions, WebpackConfig } from "../config/webpackCompat";
import { createHotReloader } from "../core/hmr";
import { BundleOptions } from "../core/types";
import { blockStdout, getPackPath } from "../utils/common";
import { findRootDir } from "../utils/find-root";
import { createSelfSignedCertificate } from "../utils/mkcert";
import { printServerInfo } from "../utils/print-server-info";
import { xcodeProfilingReady } from "../utils/xcodeProfile";

export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions,
) {
  const bundleOptions = resolveBundleOptions(options, projectPath, rootPath);

  if (!rootPath) {
    // help user to find the rootDir automatically
    rootPath = findRootDir(projectPath || process.cwd());
  }
  return serveInternal(bundleOptions, projectPath, rootPath, serverOptions);
}

async function serveInternal(
  options: BundleOptions,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions,
) {
  blockStdout();

  if (process.env.XCODE_PROFILE) {
    await xcodeProfilingReady();
  }

  // FIXME: fix any type
  const cfgDevServer = (options.config?.devServer || {}) as any;

  const serverOpts: StartServerOptions = {
    hostname: serverOptions?.hostname || cfgDevServer.host || "localhost",
    port:
      typeof serverOptions?.port !== "undefined"
        ? serverOptions!.port
        : cfgDevServer.port || 3000,
    https:
      typeof serverOptions?.https !== "undefined"
        ? serverOptions!.https
        : cfgDevServer.https,
    logServerInfo: serverOptions?.logServerInfo,
    selfSignedCertificate: serverOptions?.selfSignedCertificate,
  } as StartServerOptions;

  // If HTTPS is requested and no certificate provided, attempt to generate one.
  if (serverOpts.https && !serverOpts.selfSignedCertificate) {
    try {
      // createSelfSignedCertificate may return undefined on failure
      const cert = await createSelfSignedCertificate(serverOpts.hostname);
      if (cert) serverOpts.selfSignedCertificate = cert;
    } catch (e) {
      // ignore and fall back to http if certificate generation fails
    }
  }

  await startServer(
    serverOpts,
    {
      ...options,
      config: {
        ...options.config,
        devServer: {
          ...(options.config.devServer || {}),
          hot: true,
        },
      },
      packPath: getPackPath(),
    },
    projectPath || process.cwd(),
    rootPath,
  );
}

export interface SelfSignedCertificate {
  key: string;
  cert: string;
  rootCA?: string;
}

export interface StartServerOptions {
  port: number;
  https?: boolean;
  hostname?: string;
  logServerInfo?: boolean;
  selfSignedCertificate?: SelfSignedCertificate;
}

export type RequestHandler = (
  req: IncomingMessage,
  res: ServerResponse,
) => Promise<void>;

export type UpgradeHandler = (
  req: IncomingMessage,
  socket: Duplex,
  head: Buffer,
) => Promise<void>;

export type ServerInitResult = {
  requestHandler: RequestHandler;
  upgradeHandler: UpgradeHandler;
  closeUpgraded: () => void;
};

export async function startServer(
  serverOptions: StartServerOptions,
  bundleOptions: BundleOptions,
  projectPath: string,
  rootPath?: string,
): Promise<void> {
  let { port, hostname, selfSignedCertificate } = serverOptions;

  process.title = "utoopack-dev-server";
  let handlersReady = () => {};
  let handlersError = () => {};

  let handlersPromise: Promise<void> | undefined = new Promise<void>(
    (resolve, reject) => {
      handlersReady = resolve;
      handlersError = reject;
    },
  );
  let requestHandler = async (
    req: IncomingMessage,
    res: ServerResponse,
  ): Promise<void> => {
    if (handlersPromise) {
      await handlersPromise;
      return requestHandler(req, res);
    }
    throw new Error("Invariant request handler was not setup");
  };
  let upgradeHandler = async (
    req: IncomingMessage,
    socket: Duplex,
    head: Buffer,
  ): Promise<void> => {
    if (handlersPromise) {
      await handlersPromise;
      return upgradeHandler(req, socket, head);
    }
    throw new Error("Invariant upgrade handler was not setup");
  };

  async function requestListener(req: IncomingMessage, res: ServerResponse) {
    try {
      if (handlersPromise) {
        await handlersPromise;
        handlersPromise = undefined;
      }
      await requestHandler(req, res);
    } catch (err) {
      res.statusCode = 500;
      res.end("Internal Server Error");
      console.error(`Failed to handle request for ${req.url}`);
      console.error(err);
    }
  }

  const server = selfSignedCertificate
    ? https.createServer(
        {
          key: fs.readFileSync(selfSignedCertificate.key),
          cert: fs.readFileSync(selfSignedCertificate.cert),
        },
        requestListener,
      )
    : http.createServer(requestListener);

  server.on("upgrade", async (req, socket, head) => {
    try {
      await upgradeHandler(req, socket, head);
    } catch (err) {
      socket.destroy();
      console.error(`Failed to handle request for ${req.url}`);
      console.error(err);
    }
  });

  let portRetryCount = 0;
  const originalPort = port;

  server.on("error", (err: NodeJS.ErrnoException) => {
    if (port && err.code === "EADDRINUSE" && portRetryCount < 10) {
      port += 1;
      portRetryCount += 1;
      server.listen(port, hostname);
    } else {
      console.error(`Failed to start server`);
      console.error(err);
      process.exit(1);
    }
  });

  await new Promise<void>((resolve) => {
    server.on("listening", async () => {
      const addr = server.address();
      const actualHostname = formatHostname(
        typeof addr === "object"
          ? addr?.address || hostname || "localhost"
          : addr,
      );
      const formattedHostname =
        !hostname || actualHostname === "0.0.0.0"
          ? "localhost"
          : actualHostname === "[::]"
            ? "[::1]"
            : formatHostname(hostname);
      port = typeof addr === "object" ? addr?.port || port : port;

      if (portRetryCount) {
        console.warn(
          `Port ${originalPort} is in use, using available port ${port} instead.`,
        );
      }

      if (serverOptions.logServerInfo !== false) {
        printServerInfo(
          serverOptions.https ? "https" : "http",
          formattedHostname,
          port,
        );
      }

      try {
        let cleanupStarted = false;
        let closeUpgraded: (() => void) | null = null;
        const cleanup = () => {
          if (cleanupStarted) {
            return;
          }
          cleanupStarted = true;
          (async () => {
            console.debug("start-server process cleanup");

            await new Promise<void>((res) => {
              server.close((err) => {
                if (err) console.error(err);
                res();
              });
              server.closeAllConnections();
              closeUpgraded?.();
            });

            console.debug("start-server process cleanup finished");
            process.exit(0);
          })();
        };
        const exception = (err: Error) => {
          console.error(err);
        };
        process.on("SIGINT", cleanup);
        process.on("SIGTERM", cleanup);
        process.on("rejectionHandled", () => {});
        process.on("uncaughtException", exception);
        process.on("unhandledRejection", exception);

        const initResult = await initialize(
          bundleOptions,
          projectPath,
          rootPath,
        );
        requestHandler = initResult.requestHandler;
        upgradeHandler = initResult.upgradeHandler;
        closeUpgraded = initResult.closeUpgraded;
        handlersReady();
      } catch (err) {
        handlersError();
        console.error(err);
        process.exit(1);
      }

      resolve();
    });
    server.listen(port, hostname);
  });
}

export async function initialize(
  bundleOptions: BundleOptions,
  projectPath: string,
  rootPath?: string,
): Promise<ServerInitResult> {
  process.env.NODE_ENV = "development";

  const hotReloader = await createHotReloader(
    bundleOptions,
    projectPath,
    rootPath,
  );
  await hotReloader.start();

  const requestHandlerImpl = async (
    req: IncomingMessage,
    res: ServerResponse,
  ) => {
    req.on("error", console.error);
    res.on("error", console.error);

    const handleRequest = async () => {
      if (!(req.method === "GET" || req.method === "HEAD")) {
        res.setHeader("Allow", ["GET", "HEAD"]);
        res.statusCode = 405;
        res.end();
      }

      const distRoot = path.resolve(
        projectPath,
        bundleOptions.config.output?.path || "./dist",
      );
      try {
        const reqUrl = req.url || "";
        const path = url.parse(reqUrl).pathname || "";
        return await serveStatic(req, res, path, { root: distRoot });
      } catch (err: any) {
        res.setHeader(
          "Cache-Control",
          "private, no-cache, no-store, max-age=0, must-revalidate",
        );
        res.statusCode = 404;
        res.end();
      }
    };

    try {
      await handleRequest();
    } catch (err) {
      res.statusCode = 500;
      res.end("Internal Server Error");
    }
  };

  let requestHandler: RequestHandler = requestHandlerImpl;

  const logError = async (
    type: "uncaughtException" | "unhandledRejection",
    err: Error | undefined,
  ) => {
    if (type === "unhandledRejection") {
      console.error("unhandledRejection: ", err);
    } else if (type === "uncaughtException") {
      console.error("uncaughtException: ", err);
    }
  };

  process.on("uncaughtException", logError.bind(null, "uncaughtException"));
  process.on("unhandledRejection", logError.bind(null, "unhandledRejection"));

  const upgradeHandler = async (
    req: IncomingMessage,
    socket: Duplex,
    head: Buffer,
  ) => {
    try {
      const isHMRRequest = req.url?.includes("turbopack-hmr");

      if (isHMRRequest) {
        hotReloader.onHMR(req, socket, head);
      } else {
        socket.end();
      }
    } catch (err) {
      console.error("Error handling upgrade request", err);
      socket.end();
    }
  };

  return {
    requestHandler,
    upgradeHandler,
    closeUpgraded() {
      hotReloader.close();
    },
  };
}

export async function pipeToNodeResponse(
  readable: ReadableStream<Uint8Array>,
  res: ServerResponse,
  waitUntilForEnd?: Promise<unknown>,
) {
  try {
    const { errored, destroyed } = res;
    if (errored || destroyed) return;

    const controller = createAbortController(res);

    const writer = createWriterFromResponse(res, waitUntilForEnd);

    await readable.pipeTo(writer, { signal: controller.signal });
  } catch (err: any) {
    if (isAbortError(err)) return;

    throw new Error("failed to pipe response", { cause: err });
  }
}

export function createAbortController(response: Writable): AbortController {
  const controller = new AbortController();

  response.once("close", () => {
    if (response.writableFinished) return;

    controller.abort(new ResponseAborted());
  });

  return controller;
}

export function isAbortError(e: any): e is Error & { name: "AbortError" } {
  return e?.name === "AbortError" || e?.name === ResponseAbortedName;
}

export const ResponseAbortedName = "ResponseAborted";
export class ResponseAborted extends Error {
  public readonly name = ResponseAbortedName;
}

function createWriterFromResponse(
  res: ServerResponse,
  waitUntilForEnd?: Promise<unknown>,
): WritableStream<Uint8Array> {
  let started = false;

  let drained = new DetachedPromise<void>();
  function onDrain() {
    drained.resolve();
  }
  res.on("drain", onDrain);

  res.once("close", () => {
    res.off("drain", onDrain);
    drained.resolve();
  });

  const finished = new DetachedPromise<void>();
  res.once("finish", () => {
    finished.resolve();
  });

  return new WritableStream<Uint8Array>({
    write: async (chunk) => {
      if (!started) {
        started = true;

        res.flushHeaders();
      }

      try {
        const ok = res.write(chunk);

        if ("flush" in res && typeof res.flush === "function") {
          res.flush();
        }

        if (!ok) {
          await drained.promise;

          drained = new DetachedPromise<void>();
        }
      } catch (err) {
        res.end();
        throw new Error("failed to write chunk to response", { cause: err });
      }
    },
    abort: (err) => {
      if (res.writableFinished) return;

      res.destroy(err);
    },
    close: async () => {
      if (waitUntilForEnd) {
        await waitUntilForEnd;
      }

      if (res.writableFinished) return;

      res.end();
      return finished.promise;
    },
  });
}

export class DetachedPromise<T = any> {
  public readonly resolve: (value: T | PromiseLike<T>) => void;
  public readonly reject: (reason: any) => void;
  public readonly promise: Promise<T>;

  constructor() {
    let resolve: (value: T | PromiseLike<T>) => void;
    let reject: (reason: any) => void;

    this.promise = new Promise<T>((res, rej) => {
      resolve = res;
      reject = rej;
    });

    this.resolve = resolve!;
    this.reject = reject!;
  }
}

export function serveStatic(
  req: IncomingMessage,
  res: ServerResponse,
  path: string,
  opts?: Parameters<typeof send>[2],
): Promise<void> {
  return new Promise((resolve, reject) => {
    send(req, path, opts)
      .on("directory", () => {
        const err: any = new Error("No directory access");
        err.code = "ENOENT";
        reject(err);
      })
      .on("error", reject)
      .pipe(res)
      .on("finish", resolve);
  });
}

export function formatHostname(hostname: string): string {
  return isIPv6(hostname) ? `[${hostname}]` : hostname;
}
