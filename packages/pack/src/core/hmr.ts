import {
  type CompilationError,
  type EntryIssuesMap,
  type EntryOptions,
  formatIssue,
  type HMR_ACTION_TYPES,
  HMR_ACTIONS_SENT_TO_BROWSER,
  type ReloadAction,
  type SyncAction,
  type TurbopackConnectedAction,
} from "@utoo/pack-shared";
import { IncomingMessage } from "http";
import { nanoid } from "nanoid";
import type { Socket } from "net";
import { Duplex } from "stream";
import { WebSocketServer } from "ws";
import type { MemoryEvictionMode, NapiWrittenEndpoint } from "../binding";
import { BundleOptions } from "../config/types";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { cleanOutput, getOutputPath } from "../utils/cleanOutput";
import { debounce, getPackPath, processIssues } from "../utils/common";
import { getInitialAssetsFromEndpointPaths } from "../utils/getInitialAssets";
import { processHtmlEntry } from "../utils/htmlEntry";
import { acquirePersistentCacheLock } from "../utils/lockfile";
import { normalizePath } from "../utils/normalizePath";
import { useWorkerThreads } from "../utils/runtimePluginStratety";
import { validateEntryPaths } from "../utils/validateEntry";
import { projectFactory } from "./project";
import { Endpoint, Project, Update as TurbopackUpdate } from "./types";

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
  close(): Promise<void>;
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
  clientIssues: EntryIssuesMap;
};

export type SendHmr = (id: string, payload: HMR_ACTION_TYPES) => void;

export const FAST_REFRESH_RUNTIME_RELOAD =
  "Fast Refresh had to perform a full reload due to a runtime error.";

function hasBlockingIssues(issues: EntryIssuesMap) {
  for (const issueMap of issues.values()) {
    for (const issue of issueMap.values()) {
      if (issue.severity !== "warning") {
        return true;
      }
    }
  }
  return false;
}

function hasBlockingResultIssues(result: TurbopackResult) {
  return result.issues.some(
    (issue) => issue.severity === "error" || issue.severity === "fatal",
  );
}

function addIssuesToErrors(
  errors: Map<string, CompilationError>,
  issues: EntryIssuesMap,
) {
  for (const issueMap of issues.values()) {
    for (const [key, issue] of issueMap) {
      if (issue.severity === "warning") {
        continue;
      }

      errors.set(key, {
        message: formatIssue(issue, false),
      });
    }
  }
}

function getCompilationErrors(issues: EntryIssuesMap) {
  const errors = new Map<string, CompilationError>();
  addIssuesToErrors(errors, issues);
  return [...errors.values()];
}

function getClientIssueKey(id: string) {
  return `client:${id}`;
}

export async function createHotReloader(
  bundleOptions: BundleOptions,
  projectPath?: string,
  rootPath?: string,
): Promise<HotReloaderInterface> {
  const resolvedProjectPath = projectPath || process.cwd();
  const resolvedRootPath = rootPath || projectPath || process.cwd();
  processHtmlEntry(bundleOptions.config, resolvedProjectPath);
  validateEntryPaths(bundleOptions.config, resolvedProjectPath);
  await cleanOutput(bundleOptions.config, resolvedProjectPath);

  const createProject = projectFactory();
  const persistentCaching = bundleOptions.config.persistentCaching ?? true;
  const turbopackMemoryEviction = (
    bundleOptions.config.turbopackMemoryEviction === false ? "off" : "full"
  ) as MemoryEvictionMode;
  const persistentCacheLock = await acquirePersistentCacheLock(
    resolvedProjectPath,
    "utoo pack dev",
    persistentCaching,
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
  const shouldCreateWebpackStats =
    Boolean(process.env.ANALYZE) || Boolean(bundleOptions.config.stats);

  let project: Project;
  try {
    project = await createProject(
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
          stats: shouldCreateWebpackStats,
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
        persistentCaching,
        turbopackMemoryEviction,
      },
    );
  } catch (error) {
    persistentCacheLock?.unlockSync();
    throw error;
  }

  const entrypointsSubscription = project.entrypointsSubscribe();

  let currentEntriesHandlingResolve: ((value?: unknown) => void) | undefined;
  let currentEntriesHandling = new Promise(
    (resolve) => (currentEntriesHandlingResolve = resolve),
  );

  let hmrEventHappened = false;
  let hmrHash = 0;

  const clients = new Set<WSLike>();
  const clientStates = new WeakMap<WSLike, ClientState>();
  const currentEntryIssues: EntryIssuesMap = new Map();
  const backgroundWatchSubscriptions = new Set<
    AsyncIterableIterator<TurbopackResult>
  >();

  let currentWatchedEntrypoints: Endpoint[] = [];
  let backgroundWatchersStarted = false;
  let backgroundWatchGeneration = 0;
  const backgroundEndpointWriteTasks = new Map<Endpoint, Promise<void>>();
  let backgroundProjectWriteTask: Promise<void> | undefined;
  let closed = false;
  let closePromise: Promise<void> | undefined;

  function sendToClient(client: WSLike, payload: HMR_ACTION_TYPES) {
    client.send(JSON.stringify(payload));
  }

  function sendEnqueuedMessages() {
    if (hasBlockingIssues(currentEntryIssues)) {
      return;
    }

    for (const client of clients) {
      const state = clientStates.get(client);
      if (!state) {
        continue;
      }

      if (hasBlockingIssues(state.clientIssues)) {
        return;
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
    payload.issues = [];

    for (const client of clients) {
      clientStates.get(client)?.turbopackUpdates.push(payload);
    }

    hmrEventHappened = true;
    sendEnqueuedMessagesDebounce();
  }

  const writtenEndpointPaths = new Map<Endpoint, NapiWrittenEndpoint>();

  function updateWrittenEndpointPaths(
    endpoints: Endpoint[] | undefined,
    paths: NapiWrittenEndpoint[] | undefined,
  ) {
    if (!endpoints || !paths) {
      return;
    }

    endpoints.forEach((endpoint, index) => {
      const written = paths[index];
      if (written) {
        writtenEndpointPaths.set(endpoint, written);
      }
    });
  }

  async function regenerateHtml() {
    if (htmlConfigs.length === 0) {
      return;
    }

    const outputDir = getOutputPath(bundleOptions.config, resolvedProjectPath);
    const publicPath = bundleOptions.config.output?.publicPath;
    const assets = getInitialAssetsFromEndpointPaths([
      ...writtenEndpointPaths.values(),
    ]);

    for (const config of htmlConfigs) {
      const plugin = new HtmlPlugin(config);
      await plugin.generate(outputDir, assets, publicPath);
    }
  }

  async function writeAllEntrypointsToDisk() {
    const result = await project.writeAllEntrypointsToDisk();
    processIssues(result, true, true);
    updateWrittenEndpointPaths(result.apps, result.appPaths);
    updateWrittenEndpointPaths(result.libraries, result.libraryPaths);
    await regenerateHtml();
  }

  async function writeEntrypointToDisk(entrypoint: Endpoint) {
    const result = await entrypoint.writeToDisk();
    processIssues(result, true, true);
    writtenEndpointPaths.set(entrypoint, result);
    await regenerateHtml();
  }

  async function writeOutputToDisk(entrypoint: Endpoint) {
    if (shouldCreateWebpackStats) {
      await writeAllEntrypointsToDisk();
    } else {
      await writeEntrypointToDisk(entrypoint);
    }
  }

  async function disposeBackgroundWatchSubscriptions() {
    const subscriptions = [...backgroundWatchSubscriptions];
    backgroundWatchSubscriptions.clear();
    currentEntryIssues.clear();
    backgroundEndpointWriteTasks.clear();
    backgroundProjectWriteTask = undefined;
    await Promise.all(
      subscriptions.map((subscription) => subscription.return?.()),
    );
  }

  function scheduleOutputWrite(entrypoint: Endpoint, generation: number) {
    if (
      !backgroundWatchersStarted ||
      closed ||
      generation !== backgroundWatchGeneration
    ) {
      return;
    }

    const previousTask = shouldCreateWebpackStats
      ? (backgroundProjectWriteTask ?? Promise.resolve())
      : (backgroundEndpointWriteTasks.get(entrypoint) ?? Promise.resolve());
    const task = previousTask
      .catch(() => {})
      .then(async () => {
        await currentEntriesHandling;
        if (closed || generation !== backgroundWatchGeneration) {
          return;
        }

        await writeOutputToDisk(entrypoint);
        hmrEventHappened = true;
      })
      .finally(() => {
        if (shouldCreateWebpackStats) {
          if (backgroundProjectWriteTask === task) {
            backgroundProjectWriteTask = undefined;
          }
        } else if (backgroundEndpointWriteTasks.get(entrypoint) === task) {
          backgroundEndpointWriteTasks.delete(entrypoint);
        }
      });

    if (shouldCreateWebpackStats) {
      backgroundProjectWriteTask = task;
    } else {
      backgroundEndpointWriteTasks.set(entrypoint, task);
    }
  }

  async function refreshBackgroundWatchers() {
    const generation = ++backgroundWatchGeneration;

    await disposeBackgroundWatchSubscriptions();

    if (!backgroundWatchersStarted || closed) {
      return;
    }

    await Promise.all(
      currentWatchedEntrypoints.map(async (entrypoint) => {
        const [clientChanges, serverChanges] = await Promise.all([
          entrypoint.clientChanged(),
          entrypoint.serverChanged(true),
        ]);

        if (closed || generation !== backgroundWatchGeneration) {
          await Promise.all([
            clientChanges.return?.(),
            serverChanges.return?.(),
          ]);
          return;
        }

        backgroundWatchSubscriptions.add(clientChanges);
        backgroundWatchSubscriptions.add(serverChanges);

        const watchChanges = async (
          subscription: AsyncIterableIterator<TurbopackResult>,
          issueKey: string,
        ) => {
          try {
            for await (const data of subscription) {
              if (closed || generation !== backgroundWatchGeneration) {
                return;
              }

              processIssues(currentEntryIssues, issueKey, data, false, true);

              if (hasBlockingResultIssues(data)) {
                hmrEventHappened = true;
                sendEnqueuedMessagesDebounce();
                continue;
              }

              scheduleOutputWrite(entrypoint, generation);
            }
          } catch (error) {
            if (!closed && generation === backgroundWatchGeneration) {
              console.error(error);
              process.exit(1);
            }
          } finally {
            backgroundWatchSubscriptions.delete(subscription);
            currentEntryIssues.delete(issueKey);
          }
        };

        const entrypointIndex = currentWatchedEntrypoints.indexOf(entrypoint);
        void watchChanges(
          clientChanges,
          `entrypoint:${entrypointIndex}:client`,
        );
        void watchChanges(
          serverChanges,
          `entrypoint:${entrypointIndex}:server`,
        );
      }),
    );
  }

  async function subscribeToHmrEvents(client: WSLike, id: string) {
    const state = clientStates.get(client);
    if (!state || state.subscriptions.has(id)) {
      return;
    }

    const subscription = project!.hmrEvents(id);
    state.subscriptions.set(id, subscription);
    const issueKey = getClientIssueKey(id);

    // The subscription will always emit once, which is the initial
    // computation. This is not a change, so swallow it.
    try {
      await subscription.next();

      for await (const data of subscription) {
        processIssues(state.clientIssues, issueKey, data, false, true);
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
    state.clientIssues.delete(getClientIssueKey(id));
  }

  async function handleEntrypointsSubscription() {
    for await (const entrypoints of entrypointsSubscription) {
      if (!currentEntriesHandlingResolve) {
        currentEntriesHandling = new Promise(
          // eslint-disable-next-line no-loop-func
          (resolve) => (currentEntriesHandlingResolve = resolve),
        );
      }

      currentWatchedEntrypoints = [
        ...(entrypoints.apps ?? []),
        ...(entrypoints.libraries ?? []),
      ];
      if (shouldCreateWebpackStats) {
        await writeAllEntrypointsToDisk();
      } else {
        await Promise.all(
          currentWatchedEntrypoints.map((entrypoint) =>
            writeEntrypointToDisk(entrypoint),
          ),
        );
      }

      if (backgroundWatchersStarted) {
        await refreshBackgroundWatchers();
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
          clientIssues: new Map(),
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

        const errors = getCompilationErrors(currentEntryIssues);

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
        clientIssues: new Map(),
      });

      const turbopackConnected: TurbopackConnectedAction = {
        action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_CONNECTED,
        data: { sessionId },
      };
      sendToClient(ws, turbopackConnected);

      const errors = getCompilationErrors(currentEntryIssues);
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
    async start() {
      if (backgroundWatchersStarted) {
        return;
      }

      backgroundWatchersStarted = true;
      await refreshBackgroundWatchers();
    },

    async buildFallbackError() {
      // Not implemented yet.
    },

    async close() {
      closed = true;
      const disposePromise = disposeBackgroundWatchSubscriptions();
      closePromise ??= (
        bundleOptions.config.persistentCaching
          ? project.shutdown()
          : project.onExit()
      )
        .catch((err) => {
          console.error(err);
        })
        .finally(() => {
          persistentCacheLock?.unlockSync();
        });

      for (const wsClient of clients) {
        wsClient.close();
      }
      clients.clear();

      await Promise.all([disposePromise, closePromise]);
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
          addIssuesToErrors(errors, currentEntryIssues);

          for (const client of clients) {
            const state = clientStates.get(client);
            if (!state) {
              continue;
            }

            const clientErrors = new Map(errors);
            addIssuesToErrors(clientErrors, state.clientIssues);

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
