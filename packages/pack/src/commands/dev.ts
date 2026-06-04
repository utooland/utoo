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
import { getOutputPath } from "../utils/cleanOutput";
import { blockStdout, getPackPath } from "../utils/common";
import { findRootDir } from "../utils/findRoot";
import { createSelfSignedCertificate } from "../utils/mkcert";
import { printServerInfo } from "../utils/printServerInfo";
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
  /**
   * Called after the initial dev build has been written and the HTTP server is
   * listening. Integrations can await this instead of polling the internal
   * server port.
   */
  onReady?: (context: {
    port: number;
    hostname: string;
  }) => void | Promise<void>;
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
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions,
) {
  const bundleOptions = resolveBundleOptions(options, projectPath, rootPath);

  if (!rootPath) {
    rootPath = findRootDir(projectPath || process.cwd());
  }
  return runDev(bundleOptions, projectPath, rootPath, serverOptions);
}

const HMR_PATH = "/turbopack-hmr";

async function runDev(
  options: BundleOptions,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions,
): Promise<void> {
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
  await hotReloader.start();

  const distRoot = getOutputPath(options.config, projectPathResolved);
  const publicPath = options.config?.output?.publicPath;
  // Skip prefix stripping for runtime-resolved publicPath modes and when publicPath is absent.
  const normalizedPrefix =
    publicPath && !isRuntimeResolvedPublicPath(publicPath)
      ? normalizedPublicPath(publicPath)
      : "";

  const app = new Hono();
  const { injectWebSocket, upgradeWebSocket } = createNodeWebSocket({ app });

  const rewriteRequestPath = (reqPath: string): string => {
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

  // GET handles HEAD automatically in Hono; serveStatic serves both
  app.get(
    "/*",
    serveStatic({
      root: distRoot,
      rewriteRequestPath,
    }),
  );

  app.all("*", (c) => c.body(null, 405, { Allow: "GET, HEAD" }));

  const server = honoServe({
    ...serveOptsBase,
    fetch: app.fetch,
  });
  injectWebSocket(server);

  if (!server.listening) {
    await new Promise<void>((resolve, reject) => {
      const onListening = () => {
        server.off("error", onError);
        resolve();
      };
      const onError = (error: Error) => {
        server.off("listening", onListening);
        reject(error);
      };

      server.once("listening", onListening);
      server.once("error", onError);
    });
  }

  await serverOptions?.onReady?.({
    port: serveOptsBase.port,
    hostname: serveOptsBase.hostname,
  });

  if (serverOptions?.logServerInfo !== false) {
    const scheme = serveOptsBase.serverOptions ? "https" : "http";
    const displayHost =
      serveOptsBase.hostname === "0.0.0.0"
        ? "localhost"
        : serveOptsBase.hostname;
    printServerInfo(scheme, displayHost, serveOptsBase.port);
  }

  const cleanup = () => {
    hotReloader.close();
    // We always create HTTP/1.1 server (http or https), so closeAllConnections exists; Hono's
    // ServerType union includes HTTP/2, so TS does not narrow. Use runtime check to satisfy types.
    if (
      "closeAllConnections" in server &&
      typeof server.closeAllConnections === "function"
    ) {
      server.closeAllConnections();
    }
    server.close((err) => {
      if (err) {
        console.error(err);
        process.exit(1);
      }
      process.exit(0);
    });
  };

  const exception = (err: Error) => {
    console.error(err);
  };

  process.on("SIGINT", cleanup);
  process.on("SIGTERM", cleanup);
  process.on("rejectionHandled", () => {});
  process.on("uncaughtException", exception);
  process.on("unhandledRejection", exception);
}
