import {
  type CompilationError,
  type EntryOptions,
  type HMR_ACTION_TYPES,
  HMR_ACTIONS_SENT_TO_BROWSER,
  type ReloadAction,
  type SyncAction,
  type TurbopackConnectedAction,
} from "@utoo/pack-shared";
import { IncomingMessage } from "http";
import { nanoid } from "nanoid";
import type { Socket } from "net";
import path from "path";
import { Duplex } from "stream";
import { WebSocketServer } from "ws";
import { BundleOptions } from "../config/types";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { debounce, getPackPath, processIssues } from "../utils/common";
import { getInitialAssetsFromStats } from "../utils/getInitialAssets";
import { processHtmlEntry } from "../utils/htmlEntry";
import { normalizePath } from "../utils/normalize-path";
import { useWorkerThreads } from "../utils/runtimePluginStratety";
import { validateEntryPaths } from "../utils/validateEntry";
import { projectFactory } from "./project";
import { Project, Update as TurbopackUpdate } from "./types";

const wsServer = new WebSocketServer({ noServer: true });

const sessionId = Math.floor(Number.MAX_SAFE_INTEGER * Math.random());

// Re-export HMR types from pack-shared for backward compatibility
export {
  type BuildingAction,
  type BuiltAction,
  type CompilationError,
  type HMR_ACTION_TYPES,
  HMR_ACTIONS_SENT_TO_BROWSER,
  type ReloadAction,
  type SyncAction,
  type TurbopackConnectedAction,
  type TurbopackMessageAction,
} from "@utoo/pack-shared";

export interface WebpackStats {
  hash?: string;
  startTime?: number;
  endTime?: number;
  hasErrors(): boolean;
  hasWarnings(): boolean;
  toJson(options?: any): any;
  toString(options?: any): string;
}

/** Client handle for HMR: any object with send(data) usable as Set/WeakMap key (e.g. ws WebSocket or hono WSContext). */
export interface WSLike {
  send(data: string): void;
  close(code?: number, reason?: string): void;
}

export interface HotReloaderInterface {
  turbopackProject?: Project;
  serverStats: WebpackStats | null;
  setHmrServerError(error: Error | null): void;
  clearHmrServerError(): void;
  start(): Promise<void>;
  send(action: HMR_ACTION_TYPES): void;
  /**
   * @deprecated Used by legacy dev server (dev-legacy.ts). Prefer registerClient / unregisterClient / handleClientMessage (e.g. dev.ts).
   */
  onHMR(
    req: IncomingMessage,
    socket: Duplex,
    head: Buffer,
    onUpgrade?: (client: { send(data: string): void }) => void,
  ): void;
  /** Register a WebSocket client (e.g. from @hono/node-ws upgradeWebSocket). Call unregisterClient on close. */
  registerClient(ws: WSLike): void;
  /** Unregister and cleanup subscriptions for a client. */
  unregisterClient(ws: WSLike): void;
  /** Handle a message from a client (JSON string). */
  handleClientMessage(ws: WSLike, data: string): void;
  buildFallbackError(): Promise<void>;
  close(): void;
}

export type ChangeSubscriptions = Map<
  string,
  Promise<AsyncIterableIterator<TurbopackResult>>
>;

export type ReadyIds = Set<string>;

export type StartBuilding = (id: string, forceRebuild: boolean) => () => void;

export type ClientState = {
  hmrPayloads: Map<string, HMR_ACTION_TYPES>;
  turbopackUpdates: TurbopackUpdate[];
  subscriptions: Map<string, AsyncIterator<any>>;
};

export type SendHmr = (id: string, payload: HMR_ACTION_TYPES) => void;

export const FAST_REFRESH_RUNTIME_RELOAD =
  "Fast Refresh had to perform a full reload due to a runtime error.";

export async function createHotReloader(
  bundleOptions: BundleOptions,
  projectPath?: string,
  rootPath?: string,
): Promise<HotReloaderInterface> {
  const resolvedProjectPath = projectPath || process.cwd();
  const resolvedRootPath = rootPath || projectPath || process.cwd();
  processHtmlEntry(bundleOptions.config, resolvedProjectPath);
  validateEntryPaths(bundleOptions.config, resolvedProjectPath);

  const createProject = projectFactory();

  const project = await createProject(
    {
      processEnv: bundleOptions.processEnv ?? {},
      watch: {
        enable: true,
      },
      dev: true,
      buildId: bundleOptions.buildId || nanoid(),
      tracing: bundleOptions.tracing ?? true,
      config: {
        ...bundleOptions.config,
        mode: "development",
        stats:
          Boolean(process.env.ANALYZE) ||
          bundleOptions.config.stats ||
          bundleOptions.config.entry.some((e: EntryOptions) => !!e.html),
        optimization: {
          ...bundleOptions.config.optimization,
          minify: false,
          moduleIds: "named",
        },
        persistentCaching: bundleOptions?.config?.persistentCaching ?? true,
        pluginRuntimeStrategy:
          bundleOptions?.config?.pluginRuntimeStrategy ??
          (useWorkerThreads() ? "workerThreads" : "childProcesses"),
      },
      projectPath: normalizePath(resolvedProjectPath),
      rootPath: resolvedRootPath,
      packPath: getPackPath(),
    },
    {
      persistentCaching: bundleOptions.config.persistentCaching ?? false,
    },
  );

  const entrypointsSubscription = project.entrypointsSubscribe();

  let currentEntriesHandlingResolve: ((value?: unknown) => void) | undefined;
  let currentEntriesHandling = new Promise(
    (resolve) => (currentEntriesHandlingResolve = resolve),
  );

  let hmrEventHappened = false;
  let hmrHash = 0;

  const clients = new Set<WSLike>();
  const clientStates = new WeakMap<WSLike, ClientState>();

  function sendToClient(client: WSLike, payload: HMR_ACTION_TYPES) {
    client.send(JSON.stringify(payload));
  }

  function sendEnqueuedMessages() {
    for (const client of clients) {
      const state = clientStates.get(client);
      if (!state) {
        continue;
      }

      for (const payload of state.hmrPayloads.values()) {
        sendToClient(client, payload);
      }
      state.hmrPayloads.clear();

      if (state.turbopackUpdates.length > 0) {
        sendToClient(client, {
          action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_MESSAGE,
          data: state.turbopackUpdates,
        });
        state.turbopackUpdates.length = 0;
      }
    }
  }
  const sendEnqueuedMessagesDebounce = debounce(sendEnqueuedMessages, 2);

  function sendTurbopackMessage(payload: TurbopackUpdate) {
    payload.diagnostics = [];
    payload.issues = [];

    for (const client of clients) {
      clientStates.get(client)?.turbopackUpdates.push(payload);
    }

    hmrEventHappened = true;
    sendEnqueuedMessagesDebounce();
  }

  async function subscribeToHmrEvents(client: WSLike, id: string) {
    const state = clientStates.get(client);
    if (!state || state.subscriptions.has(id)) {
      return;
    }

    const subscription = project!.hmrEvents(id);
    state.subscriptions.set(id, subscription);

    // The subscription will always emit once, which is the initial
    // computation. This is not a change, so swallow it.
    try {
      await subscription.next();

      for await (const data of subscription) {
        processIssues(data, true, true);
        if (data.type !== "issues") {
          sendTurbopackMessage(data);
        }
      }
    } catch (e) {
      // The client might be using an HMR session from a previous server, tell them
      // to fully reload the page to resolve the issue. We can't use
      // `hotReloader.send` since that would force every connected client to
      // reload, only this client is out of date.
      const reloadAction: ReloadAction = {
        action: HMR_ACTIONS_SENT_TO_BROWSER.RELOAD,
        data: `error in HMR event subscription for ${id}: ${e}`,
      };
      sendToClient(client, reloadAction);
      client.close();
      return;
    }
  }

  function unsubscribeFromHmrEvents(client: WSLike, id: string) {
    const state = clientStates.get(client);
    if (!state) {
      return;
    }

    const subscription = state.subscriptions.get(id);
    subscription?.return!();
  }

  async function handleEntrypointsSubscription() {
    for await (const entrypoints of entrypointsSubscription) {
      if (!currentEntriesHandlingResolve) {
        currentEntriesHandling = new Promise(
          // eslint-disable-next-line no-loop-func
          (resolve) => (currentEntriesHandlingResolve = resolve),
        );
      }

      const assets = { js: [] as string[], css: [] as string[] };
      await Promise.all(
        [...entrypoints.apps, ...entrypoints.libraries].map((l) =>
          l.writeToDisk().then((res) => {
            processIssues(res, true, true);
          }),
        ),
      );

      const htmlConfigs = [
        ...(Array.isArray((bundleOptions.config as any).html)
          ? (bundleOptions.config as any).html
          : (bundleOptions.config as any).html
            ? [(bundleOptions.config as any).html]
            : []),
        ...bundleOptions.config.entry
          .filter((e: EntryOptions) => !!e.html)
          .map((e: EntryOptions) => e.html!),
      ];

      if (htmlConfigs.length > 0) {
        const outputDir =
          bundleOptions.config.output?.path || path.join(process.cwd(), "dist");
        const publicPath = bundleOptions.config.output?.publicPath;

        if (assets.js.length === 0 && assets.css.length === 0) {
          const discovered = getInitialAssetsFromStats(outputDir);
          assets.js.push(...discovered.js);
          assets.css.push(...discovered.css);
        }

        for (const config of htmlConfigs) {
          const plugin = new HtmlPlugin(config);
          await plugin.generate(outputDir, assets, publicPath);
        }
      }

      currentEntriesHandlingResolve!();
      currentEntriesHandlingResolve = undefined;
    }
  }

  const hotReloader: HotReloaderInterface = {
    turbopackProject: project,
    serverStats: null,

    onHMR(req, socket: Socket, head, onUpgrade) {
      wsServer.handleUpgrade(req, socket, head, (client) => {
        onUpgrade?.(client);
        const subscriptions: Map<string, AsyncIterator<any>> = new Map();

        clients.add(client);
        clientStates.set(client, {
          hmrPayloads: new Map(),
          turbopackUpdates: [],
          subscriptions,
        });

        client.on("close", () => {
          // Remove active subscriptions
          for (const subscription of subscriptions.values()) {
            subscription.return?.();
          }
          clientStates.delete(client);
          clients.delete(client);
        });

        client.addEventListener("message", ({ data }) => {
          const parsedData = JSON.parse(
            typeof data !== "string" ? data.toString() : data,
          );

          // messages
          switch (parsedData.event) {
            case "client-error": // { errorCount, clientId }
            case "client-warning": // { warningCount, clientId }
            case "client-success": // { clientId }
            case "client-full-reload": // { stackTrace, hadRuntimeError }
              const { hadRuntimeError, dependencyChain } = parsedData;
              if (hadRuntimeError) {
                console.warn(FAST_REFRESH_RUNTIME_RELOAD);
              }
              if (
                Array.isArray(dependencyChain) &&
                typeof dependencyChain[0] === "string"
              ) {
                const cleanedModulePath = dependencyChain[0]
                  .replace(/^\[project\]/, ".")
                  .replace(/ \[.*\] \(.*\)$/, "");
                console.warn(
                  `Fast Refresh had to perform a full reload when ${cleanedModulePath} changed.`,
                );
              }
              break;

            default:
              // Might be a Turbopack message...
              if (!parsedData.type) {
                throw new Error(`unrecognized HMR message "${data}"`);
              }
          }

          // Turbopack messages
          switch (parsedData.type) {
            case "turbopack-subscribe":
              subscribeToHmrEvents(client, parsedData.path);
              break;

            case "turbopack-unsubscribe":
              unsubscribeFromHmrEvents(client, parsedData.path);
              break;

            default:
              if (!parsedData.event) {
                throw new Error(`unrecognized Turbopack HMR message "${data}"`);
              }
          }
        });

        const turbopackConnected: TurbopackConnectedAction = {
          action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_CONNECTED,
          data: { sessionId },
        };
        sendToClient(client, turbopackConnected);

        const errors: CompilationError[] = [];

        (async function () {
          const sync: SyncAction = {
            action: HMR_ACTIONS_SENT_TO_BROWSER.SYNC,
            errors,
            warnings: [],
            hash: "",
          };

          sendToClient(client, sync);
        })();
      });
    },

    registerClient(ws) {
      const subscriptions: Map<string, AsyncIterator<any>> = new Map();
      clients.add(ws);
      clientStates.set(ws, {
        hmrPayloads: new Map(),
        turbopackUpdates: [],
        subscriptions,
      });

      const turbopackConnected: TurbopackConnectedAction = {
        action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_CONNECTED,
        data: { sessionId },
      };
      sendToClient(ws, turbopackConnected);

      const errors: CompilationError[] = [];
      const sync: SyncAction = {
        action: HMR_ACTIONS_SENT_TO_BROWSER.SYNC,
        errors,
        warnings: [],
        hash: "",
      };
      sendToClient(ws, sync);
    },

    unregisterClient(ws) {
      const state = clientStates.get(ws);
      if (state) {
        for (const subscription of state.subscriptions.values()) {
          subscription.return?.();
        }
      }
      clientStates.delete(ws);
      clients.delete(ws);
    },

    handleClientMessage(ws, data) {
      const parsedData = JSON.parse(data);

      switch (parsedData.event) {
        case "client-error":
        case "client-warning":
        case "client-success":
        case "client-full-reload": {
          const { hadRuntimeError, dependencyChain } = parsedData;
          if (hadRuntimeError) {
            console.warn(FAST_REFRESH_RUNTIME_RELOAD);
          }
          if (
            Array.isArray(dependencyChain) &&
            typeof dependencyChain[0] === "string"
          ) {
            const cleanedModulePath = dependencyChain[0]
              .replace(/^\[project\]/, ".")
              .replace(/ \[.*\] \(.*\)$/, "");
            console.warn(
              `Fast Refresh had to perform a full reload when ${cleanedModulePath} changed.`,
            );
          }
          break;
        }
        default:
          if (!parsedData.type) {
            throw new Error(`unrecognized HMR message "${data}"`);
          }
      }

      switch (parsedData.type) {
        case "turbopack-subscribe":
          subscribeToHmrEvents(ws, parsedData.path);
          break;
        case "turbopack-unsubscribe":
          unsubscribeFromHmrEvents(ws, parsedData.path);
          break;
        default:
          if (!parsedData.event) {
            throw new Error(`unrecognized Turbopack HMR message "${data}"`);
          }
      }
    },

    send(action) {
      const payload = JSON.stringify(action);
      for (const client of clients) {
        client.send(payload);
      }
    },

    setHmrServerError(_error) {
      // Not implemented yet.
    },
    clearHmrServerError() {
      // Not implemented yet.
    },
    async start() {},

    async buildFallbackError() {
      // Not implemented yet.
    },

    close() {
      for (const wsClient of clients) {
        wsClient.close();
      }
      clients.clear();
    },
  };

  handleEntrypointsSubscription().catch((err) => {
    console.error(err);
    process.exit(1);
  });

  // Write empty manifests
  await currentEntriesHandling;

  async function handleProjectUpdates() {
    for await (const updateMessage of project.updateInfoSubscribe(30)) {
      switch (updateMessage.updateType) {
        case "start": {
          hotReloader.send({ action: HMR_ACTIONS_SENT_TO_BROWSER.BUILDING });
          break;
        }
        case "end": {
          sendEnqueuedMessages();

          const errors = new Map<string, CompilationError>();

          for (const client of clients) {
            const state = clientStates.get(client);
            if (!state) {
              continue;
            }

            const clientErrors = new Map(errors);

            sendToClient(client, {
              action: HMR_ACTIONS_SENT_TO_BROWSER.BUILT,
              hash: String(++hmrHash),
              errors: [...clientErrors.values()],
              warnings: [],
            });
          }

          if (hmrEventHappened) {
            const time = updateMessage.value!.duration;
            const timeMessage =
              time > 2000 ? `${Math.round(time / 100) / 10}s` : `${time}ms`;
            console.log(`Compiled in ${timeMessage}`);
            hmrEventHappened = false;
          }
          break;
        }
        default:
      }
    }
  }

  handleProjectUpdates().catch((err) => {
    console.error(err);
    process.exit(1);
  });

  return hotReloader;
}
