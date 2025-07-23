import { IncomingMessage } from "http";
import { nanoid } from "nanoid";
import type { Socket } from "net";
import { Duplex } from "stream";
import type webpack from "webpack";
import ws from "ws";
import { projectFactory } from "./project";
import { BundleOptions, Project, Update as TurbopackUpdate } from "./types";
import { createDefineEnv, debounce, processIssues } from "./util";

const wsServer = new ws.Server({ noServer: true });

const sessionId = Math.floor(Number.MAX_SAFE_INTEGER * Math.random());

export const enum HMR_ACTIONS_SENT_TO_BROWSER {
  RELOAD = "reload",
  CLIENT_CHANGES = "clientChanges",
  SERVER_ONLY_CHANGES = "serverOnlyChanges",
  SYNC = "sync",
  BUILT = "built",
  BUILDING = "building",
  TURBOPACK_MESSAGE = "turbopack-message",
  TURBOPACK_CONNECTED = "turbopack-connected",
}

export interface TurbopackMessageAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_MESSAGE;
  data: TurbopackUpdate | TurbopackUpdate[];
}

export interface TurbopackConnectedAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_CONNECTED;
  data: { sessionId: number };
}

interface BuildingAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.BUILDING;
}

export interface CompilationError {
  moduleName?: string;
  message: string;
  details?: string;
  moduleTrace?: Array<{ moduleName?: string }>;
  stack?: string;
}

export interface SyncAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.SYNC;
  hash: string;
  errors: ReadonlyArray<CompilationError>;
  warnings: ReadonlyArray<CompilationError>;
  updatedModules?: ReadonlyArray<string>;
}

export interface BuiltAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.BUILT;
  hash: string;
  errors: ReadonlyArray<CompilationError>;
  warnings: ReadonlyArray<CompilationError>;
  updatedModules?: ReadonlyArray<string>;
}

export interface ReloadAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.RELOAD;
  data: string;
}

export type HMR_ACTION_TYPES =
  | TurbopackMessageAction
  | TurbopackConnectedAction
  | BuildingAction
  | SyncAction
  | BuiltAction
  | ReloadAction;

export interface HotReloaderInterface {
  turbopackProject?: Project;
  serverStats: webpack.Stats | null;
  setHmrServerError(error: Error | null): void;
  clearHmrServerError(): void;
  start(): Promise<void>;
  send(action: HMR_ACTION_TYPES): void;
  onHMR(
    req: IncomingMessage,
    socket: Duplex,
    head: Buffer,
    onUpgrade?: (client: { send(data: string): void }) => void,
  ): void;
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
  const createProject = projectFactory();

  const project = await createProject(
    {
      processEnv: bundleOptions.processEnv ?? {},
      defineEnv: createDefineEnv({
        config: bundleOptions.config,
        dev: true,
        optionDefineEnv: bundleOptions.defineEnv,
      }),
      watch: {
        enable: true,
      },
      dev: true,
      buildId: bundleOptions.buildId || nanoid(),
      config: {
        ...bundleOptions.config,
        mode: "development",
        optimization: {
          ...bundleOptions.config.optimization,
          minify: false,
          moduleIds: "named",
        },
      },
      projectPath: projectPath || process.cwd(),
      rootPath: rootPath || projectPath || process.cwd(),
    },
    {
      persistentCaching: true,
    },
  );

  const entrypointsSubscription = project.entrypointsSubscribe();

  let currentEntriesHandlingResolve: ((value?: unknown) => void) | undefined;
  let currentEntriesHandling = new Promise(
    (resolve) => (currentEntriesHandlingResolve = resolve),
  );

  let hmrEventHappened = false;
  let hmrHash = 0;

  const clients = new Set<ws>();
  const clientStates = new WeakMap<ws, ClientState>();

  function sendToClient(client: ws, payload: HMR_ACTION_TYPES) {
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

  async function subscribeToHmrEvents(client: ws, id: string) {
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

  function unsubscribeFromHmrEvents(client: ws, id: string) {
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

      await Promise.all(
        entrypoints.apps.map((l) =>
          l.writeToDisk().then((res) => processIssues(res, true, true)),
        ),
      );

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
        // it's okay to not cleanly close these websocket connections, this is dev
        wsClient.terminate();
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
