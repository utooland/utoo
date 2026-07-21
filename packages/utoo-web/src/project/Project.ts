import {
  type ConfigComplete,
  HMR_ACTIONS_SENT_TO_BROWSER,
  handleIssues,
  type TurbopackUpdate,
  type UpdateMessage,
} from "@utoo/pack-shared";
import * as comlink from "comlink";
import { HmrClient, HmrServer } from "../hmr";
import { WorkerMessageType } from "../message";
import { installServiceWorker } from "../serviceWorker/install";
import {
  BuildOutput,
  DepsOptions,
  Dirent,
  HmrSubscribeOptions,
  InstallOptions,
  PackFile,
  ProjectEndpoint,
  ProjectOptions,
  RawDirent,
  RawStats,
  ServiceWorkerOptions,
  Stats,
} from "../types";
import { createWorkerFromDataUri } from "../workers/inline";
import { checkCompatibility } from "./checkCompatibility";
import { ForkedProject } from "./ForkedProject";

let ProjectWorker: Worker;

const ConnectedPorts = new Set<MessagePort>();

type RemoteProjectEndpoint = ProjectEndpoint & {
  hmrUnsubscribe: NonNullable<ProjectEndpoint["hmrUnsubscribe"]>;
};

export class Project implements ProjectEndpoint {
  #mount: Promise<void>;

  public readonly cwd: string;

  public readonly serviceWorkerOptions?: ServiceWorkerOptions;

  /** HMR server for managing hot module replacement with preview iframes */
  private hmrServer?: HmrServer;

  private remote: comlink.Remote<
    RemoteProjectEndpoint & {
      mount: (
        opt: Omit<ProjectOptions, "workerUrl" | "serviceWorker">,
      ) => Promise<void>;
    }
  >;

  constructor(private options: ProjectOptions) {
    const {
      cwd,
      workerUrl,
      wasmUrl,
      threadWorkerUrl,
      serviceWorker,
      logFilter,
      loadersImportMap,
    } = options;

    this.cwd = cwd;
    this.serviceWorkerOptions = serviceWorker;

    const { port1, port2 } = new MessageChannel();

    this.remote ??= comlink.wrap(port1);

    if (!ProjectWorker) {
      ProjectWorker = workerUrl.startsWith("data:")
        ? createWorkerFromDataUri(workerUrl)
        : new Worker(workerUrl);
      window.addEventListener("message", (e) => {
        this.connectWorker(e);
      });

      if (this.serviceWorkerOptions) {
        navigator.serviceWorker.addEventListener("message", (e) => {
          this.connectWorker(e);
        });
      }
    }

    ProjectWorker.postMessage(WorkerMessageType.InitConnection, [port2]);

    this.#mount ??= this.remote.mount({
      cwd,
      wasmUrl,
      threadWorkerUrl,
      loaderWorkerUrl: this.options.loaderWorkerUrl,
      loadersImportMap,
      logFilter,
    });
  }

  private connectWorker(e: MessageEvent) {
    const port = e.ports[0];
    if (e.data === WorkerMessageType.RequestFork && !ConnectedPorts.has(port)) {
      ProjectWorker.postMessage(WorkerMessageType.InitConnection, [port]);
    }
  }

  public async installServiceWorker() {
    if (this.serviceWorkerOptions) {
      const { url, scope, targetDirToCwd } = this.serviceWorkerOptions;
      // Should add "Service-Worker-Allowed": "/" in page root response headers,
      return await installServiceWorker(url, scope, targetDirToCwd);
    }
  }

  public async mount() {
    return await this.#mount;
  }

  public async deps(options?: DepsOptions) {
    await this.#mount;
    return await this.remote.deps(options);
  }

  public async install(packageLock: string, options?: InstallOptions) {
    await this.#mount;
    return await this.remote.install(packageLock, options);
  }

  public async build(options?: {
    config?: ConfigComplete;
    cleanup?: boolean;
  }): Promise<BuildOutput> {
    await this.#mount;
    const res = await this.remote.build(options);
    handleIssues(res.issues, false, false);
    return res;
  }

  public async dev(options?: {
    config?: ConfigComplete;
    onUpdate?: (result: BuildOutput) => void;
  }): Promise<void> {
    await this.#mount;

    // Create HmrServer lazily on first dev() call
    if (!this.hmrServer) {
      this.hmrServer = new HmrServer({
        onSubscribe: async (path, client, expectedVersion, validation) => {
          await this.hmrSubscribe(
            path,
            (update) => {
              client.send({
                action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_MESSAGE,
                data: update as TurbopackUpdate,
              });
            },
            {
              expectedVersion,
              subscriptionId: `${client.id}:${path}`,
              validation,
            },
          );
        },
        onUnsubscribe: (path, client) => {
          void this.hmrUnsubscribe(`${client.id}:${path}`);
        },
      });
    }

    // Pass config and onUpdate as separate top-level args for Comlink serialization
    (this.remote.dev as any)(
      options?.config,
      options?.onUpdate
        ? comlink.proxy((result: BuildOutput) => {
            handleIssues(result.issues, false, false);
            options.onUpdate!(result);
          })
        : undefined,
    );
  }

  public async hmrSubscribe(
    identifier: string,
    callback: (update: unknown) => void,
    options?: HmrSubscribeOptions,
  ): Promise<void> {
    await this.remote.hmrSubscribe(
      identifier,
      comlink.proxy(callback),
      options,
    );
  }

  public async hmrUnsubscribe(subscriptionId: string): Promise<void> {
    await this.remote.hmrUnsubscribe(subscriptionId);
  }

  public updateInfoSubscribe(
    aggregationMs: number,
    callback: (message: UpdateMessage) => void,
  ): void {
    this.remote.updateInfoSubscribe(aggregationMs, comlink.proxy(callback));
  }

  public async readFile(path: string, encoding?: "utf8") {
    await this.#mount;
    return (await this.remote.readFile(path, encoding)) as any;
  }

  public async writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ) {
    await this.#mount;
    if (content instanceof Uint8Array) {
      return await this.remote.writeFile(path, content, encoding);
    }
    return await this.remote.writeFile(path, content, encoding);
  }

  public async copyFile(src: string, dst: string) {
    await this.#mount;
    return await this.remote.copyFile(src, dst);
  }

  public async readdir(
    path: string,
    options?: { recursive?: boolean },
  ): Promise<Dirent[]> {
    await this.#mount;
    const dirEntry = (await this.remote.readdir(
      path,
      options,
    )) as any as RawDirent[];
    return dirEntry.map((e) => new Dirent(e));
  }

  public async mkdir(path: string, options?: { recursive?: boolean }) {
    await this.#mount;
    return await this.remote.mkdir(path, options);
  }

  public async rm(path: string, options?: { recursive?: boolean }) {
    await this.#mount;
    return await this.remote.rm(path, options);
  }

  public async rmdir(path: string, options?: { recursive?: boolean }) {
    await this.#mount;
    return await this.remote.rmdir(path, options);
  }

  public async stat(path: string): Promise<Stats> {
    await this.#mount;
    const raw = (await this.remote.stat(path)) as any as RawStats;
    return new Stats(raw);
  }

  public async gzip(files: PackFile[]): Promise<Uint8Array> {
    await this.#mount;
    return await this.remote.gzip(files);
  }

  public async sigMd5(content: Uint8Array): Promise<string> {
    await this.#mount;
    return await this.remote.sigMd5(content);
  }

  /**
   * Connect an iframe to the HMR server for hot module replacement.
   * Only works after dev() has been called.
   *
   * @param iframe The iframe element to connect
   * @param origin Optional origin for postMessage (default: "*")
   * @returns The HMR client instance, or null if connection failed or dev() not called
   */
  public connectHmrIframe(
    iframe: HTMLIFrameElement,
    origin?: string,
  ): HmrClient | null {
    if (!this.hmrServer) {
      return null;
    }
    return this.hmrServer.connectIframe(iframe, origin);
  }

  public static fork(
    channel: MessageChannel,
    eventSource?: Client | DedicatedWorkerGlobalScope,
  ): ProjectEndpoint {
    (eventSource || (self as DedicatedWorkerGlobalScope)).postMessage(
      WorkerMessageType.RequestFork,
      {
        transfer: [channel.port2],
      },
    );

    return new ForkedProject(channel.port1);
  }

  public static checkCompatibility() {
    return checkCompatibility();
  }
}
