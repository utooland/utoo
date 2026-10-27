/**
 * Dev server implementation using Hono + @hono/node-server + @hono/node-ws.
 * Keeps the same public API as dev.ts; do not remove dev.ts until this is verified.
 */

import { serve as honoServe } from "@hono/node-server";
import { serveStatic } from "@hono/node-server/serve-static";
import { createNodeWebSocket } from "@hono/node-ws";
import type { DevServerConfig } from "@utoo/pack-shared";
import fs from "fs";
import getPort from "get-port";
import { Hono } from "hono";
import { parseAccept } from "hono/utils/accept";
import https from "https";
import path from "path";
import { BundleOptions } from "../config/types";
import {
  resolveBundleOptions,
  type WebpackConfig,
} from "../config/webpackCompat";
import type { HotReloaderInterface } from "../core/hmr";
import { createHotReloader } from "../core/hmr";
import { createHttpProxyMiddleware } from "../core/proxyHono";
import type { Project } from "../core/types";
import { getOutputPath } from "../utils/cleanOutput";
import { blockStdout, getPackPath } from "../utils/common";
import { createSelfSignedCertificate } from "../utils/mkcert";
import { printServerInfo } from "../utils/printServerInfo";
import { resolveRootPath } from "../utils/resolveRootPath";
import { xcodeProfilingReady } from "../utils/xcodeProfile";

// --- Path helpers (same logic as dev.ts, not exported) ---

function parsePath(pathStr: string): {
  pathname: string;
  query: string;
  hash: string;
} {
  const hashIndex = pathStr.indexOf("#");
  const queryIndex = pathStr.indexOf("?");
  const hasQuery = queryIndex > -1 && (hashIndex < 0 || queryIndex < hashIndex);

  if (hasQuery || hashIndex > -1) {
    return {
      pathname: pathStr.substring(0, hasQuery ? queryIndex : hashIndex),
      query: hasQuery
        ? pathStr.substring(queryIndex, hashIndex > -1 ? hashIndex : undefined)
        : "",
      hash: hashIndex > -1 ? pathStr.slice(hashIndex) : "",
    };
  }

  return { pathname: pathStr, query: "", hash: "" };
}

function pathHasPrefix(pathStr: string, prefix: string): boolean {
  if (typeof pathStr !== "string") {
    return false;
  }
  const { pathname } = parsePath(pathStr);
  return pathname === prefix || pathname.startsWith(prefix + "/");
}

function removePathPrefix(pathStr: string, prefix: string): string {
  if (!pathHasPrefix(pathStr, prefix)) {
    return pathStr;
  }
  const withoutPrefix = pathStr.slice(prefix.length);
  if (withoutPrefix.startsWith("/")) {
    return withoutPrefix;
  }
  return `/${withoutPrefix}`;
}

function normalizedPublicPath(publicPath: string | undefined): string {
  const escapedPublicPath = publicPath?.replace(/^\/+|\/+$/g, "") || false;

  if (!escapedPublicPath) {
    return "";
  }

  try {
    if (URL.canParse(escapedPublicPath)) {
      const u = new URL(escapedPublicPath).toString();
      return u.endsWith("/") ? u.slice(0, -1) : u;
    }
  } catch {}

  return `/${escapedPublicPath}`;
}

function isRuntimeResolvedPublicPath(publicPath: string | undefined): boolean {
  return publicPath === "runtime" || publicPath === "auto";
}

// --- Public types (match dev.ts for index switch) ---

// The `.<hex>` suffix produced by `[contenthash]` filename templates. Turbopack's
// default `_<hash>` suffix is derived from the asset ident, so it stays the same
// when a manifest chunk is rewritten and needs no stripping.
const CONTENT_HASH_SUFFIX = /\.[0-9a-f]{6,}(\.js)$/;

/**
 * Finds the most recently written chunk in `distRoot` that shares the
 * content-hash-less stem of `chunkPath` (relative to `distRoot`).
 */
async function latestChunkWithSameStem(
  distRoot: string,
  chunkPath: string,
): Promise<string | null> {
  const dir = path.posix.dirname(chunkPath);
  const stem = path.posix
    .basename(chunkPath)
    .replace(CONTENT_HASH_SUFFIX, "$1");
  const absDir = path.join(distRoot, dir);
  let entries: string[];
  try {
    entries = await fs.promises.readdir(absDir);
  } catch {
    return null;
  }
  let latest: { name: string; mtimeMs: number } | null = null;
  for (const name of entries) {
    if (name.replace(CONTENT_HASH_SUFFIX, "$1") !== stem) continue;
    const { mtimeMs } = await fs.promises.stat(path.join(absDir, name));
    if (!latest || mtimeMs > latest.mtimeMs) latest = { name, mtimeMs };
  }
  return latest ? path.posix.join(dir, latest.name) : null;
}

export interface SelfSignedCertificate {
  key: string;
  cert: string;
  rootCA?: string;
}

export interface DevServerReadyContext {
  port: number;
  hostname: string;
  /** Initial client asset paths written for all configured endpoints. */
  clientPaths: string[];
}

export interface DevServerHandle {
  port: number;
  hostname: string;
  /** Write current entrypoints and their HTML before an integration publishes its build. */
  writeAllEntrypointsToDisk: Project["writeAllEntrypointsToDisk"];
  /** Close the HTTP server, HMR connections and project without exiting the process. */
  close(): Promise<void>;
}

export interface StartServerOptions {
  port: number;
  https?: boolean;
  hostname?: string;
  logServerInfo?: boolean;
  /** Serve index.html for unmatched SPA routes. Defaults to false; the CLI enables it. */
  historyApiFallback?: boolean;
  /** Handle process signals and errors. Defaults to true; embedded servers can opt out. */
  handleSignals?: boolean;
  selfSignedCertificate?: SelfSignedCertificate;
  /**
   * Called after the initial dev build has been written and the HTTP server is
   * listening. Integrations can await this instead of polling the internal
   * server port.
   */
  onReady?: (context: DevServerReadyContext) => void | Promise<void>;
}

/** Options for honoServe excluding fetch; we only use HTTP or HTTPS (no HTTP/2). */
interface ServeOptsBase {
  port: number;
  hostname: string;
  createServer?: typeof https.createServer;
  serverOptions?: { key: Buffer; cert: Buffer };
}

interface ResolvedDevConfig {
  bundleOptions: BundleOptions;
  projectPathResolved: string;
  rootPathResolved: string;
  serveOptsBase: ServeOptsBase;
}

async function resolveDevConfig(
  options: BundleOptions,
  projectPath: string | undefined,
  rootPath: string | undefined,
  serverOptions: StartServerOptions | undefined,
): Promise<ResolvedDevConfig> {
  const cfgDevServer: DevServerConfig = options.config?.devServer ?? {};
  const port =
    typeof serverOptions?.port !== "undefined"
      ? serverOptions.port
      : (cfgDevServer.port ?? 3000);
  const hostname = serverOptions?.hostname ?? cfgDevServer.host ?? "localhost";
  const useHttps =
    typeof serverOptions?.https !== "undefined"
      ? serverOptions.https
      : cfgDevServer.https;

  let selfSignedCertificate = serverOptions?.selfSignedCertificate;
  if (useHttps && !selfSignedCertificate) {
    try {
      const cert = await createSelfSignedCertificate(hostname);
      if (cert) selfSignedCertificate = cert;
    } catch {
      // fall back to http
    }
  }
  const bundleOptions: BundleOptions = {
    ...options,
    config: {
      ...options.config,
      devServer: {
        hot: true,
        ...(options.config?.devServer || {}),
      },
    },
    packPath: getPackPath(),
  };

  const projectPathResolved = projectPath || process.cwd();
  const rootPathResolved = rootPath ?? projectPathResolved;

  const actualPort = await getPort({ port, host: hostname });
  if (actualPort !== port) {
    console.warn(
      `Port ${port} is in use, using available port ${actualPort} instead.`,
    );
  }
  const serveOptsBase: ServeOptsBase = {
    port: actualPort,
    hostname,
    ...(useHttps && selfSignedCertificate
      ? {
          createServer: https.createServer,
          serverOptions: {
            key: fs.readFileSync(selfSignedCertificate.key),
            cert: fs.readFileSync(selfSignedCertificate.cert),
          },
        }
      : {}),
  };
  return {
    bundleOptions,
    projectPathResolved,
    rootPathResolved,
    serveOptsBase,
  };
}

// --- Entry ---

export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath: string | undefined,
  rootPath: string | undefined,
  serverOptions: StartServerOptions & { returnHandle: true },
): Promise<DevServerHandle>;
export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath: string | undefined,
  rootPath: string | undefined,
  serverOptions: StartServerOptions & { returnHandle: false },
): Promise<void>;
export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath: string | undefined,
  rootPath: string | undefined,
  serverOptions: StartServerOptions & { returnHandle: boolean },
): Promise<DevServerHandle | void>;
// Keep the legacy overload last for integrations using Parameters<typeof serve>.
export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions & { returnHandle?: false },
): Promise<void>;
export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions & { returnHandle?: boolean },
): Promise<DevServerHandle | void> {
  const bundleOptions = resolveBundleOptions(options, projectPath, rootPath);

  rootPath = resolveRootPath(projectPath, rootPath);
  return runDev(bundleOptions, projectPath, rootPath, serverOptions).then(
    (handle) => {
      if (serverOptions?.returnHandle) return handle;
    },
  );
}

const HMR_PATH = "/turbopack-hmr";

async function runDev(
  options: BundleOptions,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions,
): Promise<DevServerHandle> {
  blockStdout();
  process.title = "utoopack-dev-server";

  if (process.env.XCODE_PROFILE) {
    await xcodeProfilingReady();
  }

  process.env.NODE_ENV = "development";

  const {
    bundleOptions,
    projectPathResolved,
    rootPathResolved,
    serveOptsBase,
  } = await resolveDevConfig(options, projectPath, rootPath, serverOptions);

  const hotReloader: HotReloaderInterface = await createHotReloader(
    bundleOptions,
    projectPathResolved,
    rootPathResolved,
  );
  let server: ReturnType<typeof honoServe> | undefined;
  let webSocket: ReturnType<typeof createNodeWebSocket> | undefined;
  let closePromise: Promise<void> | undefined;
  const exception = (error: Error) => {
    console.error(error);
  };
  const rejectionHandled = () => {};
  const onSignal = () => {
    void handle.close().then(
      () => process.exit(0),
      (error) => {
        console.error(error);
        process.exit(1);
      },
    );
  };
  const closeResources = async () => {
    process.off("SIGINT", onSignal);
    process.off("SIGTERM", onSignal);
    process.off("rejectionHandled", rejectionHandled);
    process.off("uncaughtException", exception);
    process.off("unhandledRejection", exception);

    const closeHttp = new Promise<void>((resolve, reject) => {
      if (!server) {
        resolve();
        return;
      }
      server.close((error) => {
        if (
          error &&
          (error as NodeJS.ErrnoException).code !== "ERR_SERVER_NOT_RUNNING"
        ) {
          reject(error);
        } else {
          resolve();
        }
      });
      // Stop accepting requests before terminating existing HTTP connections.
      if (
        "closeAllConnections" in server &&
        typeof server.closeAllConnections === "function"
      ) {
        server.closeAllConnections();
      }
    });
    const closeWebSocket = new Promise<void>((resolve, reject) => {
      if (!webSocket) {
        resolve();
        return;
      }
      for (const client of webSocket.wss.clients) {
        client.terminate();
      }
      webSocket.wss.close((error) => {
        if (error) reject(error);
        else resolve();
      });
    });
    const results = await Promise.allSettled([
      closeHttp,
      closeWebSocket,
      hotReloader.close(),
    ]);
    const errors = results.flatMap((result) =>
      result.status === "rejected" ? [result.reason] : [],
    );
    if (errors.length > 0) {
      throw new AggregateError(errors, "Failed to close the dev server");
    }
  };
  const handle: DevServerHandle = {
    port: serveOptsBase.port,
    hostname: serveOptsBase.hostname,
    writeAllEntrypointsToDisk() {
      if (closePromise) {
        return Promise.reject(new Error("The dev server is closing or closed"));
      }
      return hotReloader.writeAllEntrypointsToDisk();
    },
    close() {
      closePromise ??= closeResources();
      return closePromise;
    },
  };

  try {
    await hotReloader.start();

    const distRoot = getOutputPath(options.config, projectPathResolved);
    const publicPath = options.config?.output?.publicPath;
    // Skip prefix stripping for runtime-resolved publicPath modes and when publicPath is absent.
    const normalizedPrefix =
      publicPath && !isRuntimeResolvedPublicPath(publicPath)
        ? normalizedPublicPath(publicPath)
        : "";

    const app = new Hono();
    webSocket = createNodeWebSocket({ app });
    const { injectWebSocket, upgradeWebSocket } = webSocket;

    // Manifest chunks of lazily compiled dynamic imports are rewritten under a
    // new content hash on activation; serve the browser's stable URL from there.
    const lazyChunkAliases = new Map<string, string>();
    const stripPublicPathPrefix = (reqPath: string): string => {
      if (!normalizedPrefix) return reqPath;
      // Absolute-URL publicPath: do not rewrite (match dev.ts).
      if (
        normalizedPrefix.startsWith("http://") ||
        normalizedPrefix.startsWith("https://")
      ) {
        return reqPath;
      }
      if (pathHasPrefix(reqPath, normalizedPrefix)) {
        return removePathPrefix(reqPath, normalizedPrefix);
      }
      return reqPath;
    };
    const rewriteRequestPath = (reqPath: string): string => {
      // Aliases are keyed by the output-relative chunk path, i.e. after the
      // public-path prefix is removed.
      const distPath = stripPublicPathPrefix(reqPath);
      const alias = lazyChunkAliases.get(distPath.replace(/^\/+/, ""));
      return alias ? `/${alias}` : distPath;
    };

    // HMR WebSocket route must be registered before "/*" so it is not handled by serveStatic
    app.get(
      HMR_PATH,
      upgradeWebSocket((_c) => ({
        onOpen(_ev, ws) {
          hotReloader.registerClient(ws);
        },
        onMessage(ev, ws) {
          try {
            const data =
              typeof ev.data === "string" ? ev.data : ev.data.toString();
            hotReloader.handleClientMessage(ws, data);
          } catch (err) {
            console.error("HMR message error", err);
          }
        },
        onClose(_ev, ws) {
          hotReloader.unregisterClient(ws);
        },
        onError(err) {
          console.error("HMR WebSocket error", err);
        },
      })),
    );

    const proxyRules = bundleOptions.config?.devServer?.proxy;
    if (proxyRules && proxyRules.length > 0) {
      app.use("*", createHttpProxyMiddleware(proxyRules));
    }

    if (bundleOptions.config?.devServer?.lazyDynamicImports) {
      // Requesting the manifest chunk of a lazily compiled dynamic import is what
      // activates it, so rebuild the owning entrypoints before serveStatic reads it.
      app.use("/*", async (c, next) => {
        if (
          (c.req.method === "GET" || c.req.method === "HEAD") &&
          c.req.path.includes("lazy-compilation-")
        ) {
          const chunkPath = decodeURIComponent(
            stripPublicPathPrefix(c.req.path),
          ).replace(/^\/+/, "");
          try {
            if (await hotReloader.activateLazyChunk(chunkPath)) {
              // The activated manifest has different content, so a content-hashed
              // chunk filename puts it under a new name while the browser keeps
              // requesting the URL baked into the entry. Serve the newest file
              // with the same stem for that URL.
              const current = await latestChunkWithSameStem(
                distRoot,
                chunkPath,
              );
              if (current && current !== chunkPath) {
                lazyChunkAliases.set(chunkPath, current);
              }
            }
          } catch (err) {
            console.error(err);
          }
        }
        await next();
      });
    }

    // GET handles HEAD automatically in Hono; serveStatic serves both
    app.get(
      "/*",
      serveStatic({
        root: distRoot,
        rewriteRequestPath,
      }),
    );

    // Framework integrations keep their own routing unless they explicitly opt in.
    if (serverOptions?.historyApiFallback === true) {
      const serveIndex = serveStatic({ root: distRoot, path: "index.html" });
      app.get("/*", (c, next) => {
        let pathname: string;
        try {
          pathname = decodeURIComponent(c.req.path);
        } catch {
          return next();
        }
        if (
          pathname === HMR_PATH ||
          path.posix.basename(pathname).includes(".")
        ) {
          return next();
        }

        // Both the SPA shell and the miss response depend on Accept.
        c.header("Vary", "Accept", { append: true });
        const acceptedTypes = parseAccept(
          (c.req.header("accept") || "*/*").toLowerCase(),
        );
        const htmlAccept =
          acceptedTypes.find(({ type }) => type === "text/html") ??
          acceptedTypes.find(({ type }) => type === "text/*") ??
          acceptedTypes.find(({ type }) => type === "*/*");
        if (!htmlAccept?.q || acceptedTypes[0]?.type === "application/json") {
          return next();
        }

        return serveIndex(c, next);
      });
    }

    app.all("*", (c) => {
      if (c.req.method === "GET" || c.req.method === "HEAD") {
        return c.notFound();
      }
      return c.body(null, 405, { Allow: "GET, HEAD" });
    });

    server = honoServe({
      ...serveOptsBase,
      fetch: app.fetch,
    });
    injectWebSocket(server);
    const httpServer = server;

    if (!httpServer.listening) {
      await new Promise<void>((resolve, reject) => {
        const onListening = () => {
          httpServer.off("error", onError);
          resolve();
        };
        const onError = (error: Error) => {
          httpServer.off("listening", onListening);
          reject(error);
        };

        httpServer.once("listening", onListening);
        httpServer.once("error", onError);
      });
    }

    await serverOptions?.onReady?.({
      port: serveOptsBase.port,
      hostname: serveOptsBase.hostname,
      clientPaths: hotReloader.getClientPaths(),
    });

    if (serverOptions?.logServerInfo !== false) {
      const scheme = serveOptsBase.serverOptions ? "https" : "http";
      const displayHost =
        serveOptsBase.hostname === "0.0.0.0"
          ? "localhost"
          : serveOptsBase.hostname;
      printServerInfo(scheme, displayHost, serveOptsBase.port);
    }

    if (serverOptions?.handleSignals !== false) {
      process.on("SIGINT", onSignal);
      process.on("SIGTERM", onSignal);
      process.on("rejectionHandled", rejectionHandled);
      process.on("uncaughtException", exception);
      process.on("unhandledRejection", exception);
    }
    return handle;
  } catch (error) {
    try {
      await handle.close();
    } catch (closeError) {
      throw new AggregateError(
        [error, closeError],
        "Failed to start and close the dev server",
      );
    }
    throw error;
  }
}
