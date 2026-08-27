import {
  type ConfigComplete,
  handleIssues,
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

let ProjectWorker: Worker | undefined;
let ProjectDisposal: Promise<void> | undefined;

const ConnectedPorts = new Set<MessagePort>();
const ActiveProjects = new Set<Project>();
const RUST_DISPOSE_TIMEOUT_MS = 5_000;

let listensForWindowMessages = false;
let listensForServiceWorkerMessages = false;

function connectWorker(event: MessageEvent): void {
  const port = event.ports[0];
  if (
    event.data !== WorkerMessageType.RequestFork ||
    !port ||
    !ProjectWorker ||
    ConnectedPorts.has(port)
  ) {
    return;
  }

  ConnectedPorts.add(port);
  ProjectWorker.postMessage(WorkerMessageType.InitConnection, [port]);
}

function listenForWorkerConnections(fromServiceWorker: boolean): void {
  if (!listensForWindowMessages) {
    window.addEventListener("message", connectWorker);
    listensForWindowMessages = true;
  }

  if (fromServiceWorker && !listensForServiceWorkerMessages) {
    navigator.serviceWorker.addEventListener("message", connectWorker);
    listensForServiceWorkerMessages = true;
  }
}

function stopListeningForWorkerConnections(): void {
  if (listensForWindowMessages) {
    window.removeEventListener("message", connectWorker);
    listensForWindowMessages = false;
  }

  if (listensForServiceWorkerMessages) {
    navigator.serviceWorker.removeEventListener("message", connectWorker);
    listensForServiceWorkerMessages = false;
  }

  ConnectedPorts.clear();
}

function createAbortError(): DOMException {
  return new DOMException("The project has been disposed.", "AbortError");
}

async function waitForRustDisposal(disposal: Promise<void>): Promise<void> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    const completed = await Promise.race([
      disposal.then(() => true),
      new Promise<false>((resolve) => {
        timeout = setTimeout(() => resolve(false), RUST_DISPOSE_TIMEOUT_MS);
      }),
    ]);

    if (!completed) {
      console.warn(
        `[utoo] Rust project disposal exceeded ${RUST_DISPOSE_TIMEOUT_MS}ms; terminating the worker`,
      );
    }
  } finally {
    if (timeout !== undefined) {
      clearTimeout(timeout);
    }
  }
}

export class Project implements ProjectEndpoint {
  #mount: Promise<void>;

  #disposed = false;

  readonly #disposeController = new AbortController();

  readonly #port: MessagePort;

  #disposePromise?: Promise<void>;

  public readonly cwd: string;

  public readonly serviceWorkerOptions?: ServiceWorkerOptions;

  /** HMR server for managing hot module replacement with preview iframes */
  private hmrServer?: HmrServer;

  private remote: comlink.Remote<
    ProjectEndpoint & {
      mount: (
        opt: Omit<ProjectOptions, "workerUrl" | "serviceWorker">,
      ) => Promise<void>;
      dispose: () => Promise<void>;
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

    if (ProjectDisposal) {
      throw new Error(
        "The previous project is still being disposed. Await project.dispose() before creating another project.",
      );
    }

    const { port1, port2 } = new MessageChannel();
    this.#port = port1;

    this.remote ??= comlink.wrap(port1);

    if (!ProjectWorker) {
      ProjectWorker = workerUrl.startsWith("data:")
        ? createWorkerFromDataUri(workerUrl)
        : new Worker(workerUrl);
    }
    listenForWorkerConnections(Boolean(this.serviceWorkerOptions));
    ActiveProjects.add(this);

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

  #throwIfDisposed() {
    if (this.#disposed) {
      throw createAbortError();
    }
  }

  async #raceDisposal<T>(promise: Promise<T>): Promise<T> {
    this.#throwIfDisposed();

    const { signal } = this.#disposeController;
    return await new Promise<T>((resolve, reject) => {
      const onDispose = () => reject(createAbortError());
      signal.addEventListener("abort", onDispose, { once: true });

      promise.then(
        (value) => {
          signal.removeEventListener("abort", onDispose);
          resolve(value);
        },
        (error) => {
          signal.removeEventListener("abort", onDispose);
          reject(error);
        },
      );
    });
  }

  async #call<T>(operation: () => Promise<T>): Promise<T> {
    await this.#raceDisposal(this.#mount);
    this.#throwIfDisposed();
    return await this.#raceDisposal(operation());
  }

  #beginDisposal() {
    if (this.#disposed) {
      return;
    }

    this.#disposed = true;
    this.#disposeController.abort();
    this.hmrServer?.close();
    this.hmrServer = undefined;
  }

  #releaseConnection() {
    this.remote[comlink.releaseProxy]();
    this.#port.close();
  }

  /**
   * Stop active install/build work and release the shared worker runtime.
   * Create a new Project instance before starting work on another project.
   */
  public dispose(): Promise<void> {
    if (this.#disposePromise) {
      return this.#disposePromise;
    }

    if (ProjectDisposal) {
      return ProjectDisposal;
    }

    if (this.#disposed) {
      return Promise.resolve();
    }

    const disposal = this.#disposeRuntime();
    ProjectDisposal = disposal.finally(() => {
      ProjectDisposal = undefined;
    });
    this.#disposePromise = ProjectDisposal;
    return ProjectDisposal;
  }

  async #disposeRuntime(): Promise<void> {
    // @utoo/web currently has one shared Worker/WASM runtime. Disposing it
    // invalidates every proxy connected to that runtime, not just this proxy.
    const projects = Array.from(ActiveProjects);
    for (const project of projects) {
      project.#beginDisposal();
    }

    try {
      await waitForRustDisposal(this.remote.dispose());
    } finally {
      for (const project of projects) {
        project.#releaseConnection();
      }
      ActiveProjects.clear();

      ProjectWorker?.terminate();
      ProjectWorker = undefined;
      stopListeningForWorkerConnections();
    }
  }

  public async installServiceWorker() {
    this.#throwIfDisposed();
    if (this.serviceWorkerOptions) {
      const { url, scope, targetDirToCwd } = this.serviceWorkerOptions;
      // Should add "Service-Worker-Allowed": "/" in page root response headers,
      return await installServiceWorker(url, scope, targetDirToCwd);
    }
  }

  public async mount() {
    return await this.#raceDisposal(this.#mount);
  }

  public async deps(options?: DepsOptions) {
    return await this.#call(() => this.remote.deps(options));
  }

  public async install(packageLock: string, options?: InstallOptions) {
    return await this.#call(() => this.remote.install(packageLock, options));
  }

  public async build(options?: {
    config?: ConfigComplete;
    cleanup?: boolean;
  }): Promise<BuildOutput> {
    const res = await this.#call(() => this.remote.build(options));
    handleIssues(res.issues, false, false);
    return res;
  }

  public async dev(options?: {
    config?: ConfigComplete;
    onUpdate?: (result: BuildOutput) => void;
  }): Promise<void> {
    await this.#raceDisposal(this.#mount);
    this.#throwIfDisposed();

    // Create HmrServer lazily on first dev() call
    if (!this.hmrServer) {
      this.hmrServer = new HmrServer({
        onSubscribe: async (path, client) => {
          await this.hmrSubscribe(path, (update) => {
            this.hmrServer!.sendUpdate(path, update as any);
          });
        },
      });
    }

    // Pass config and onUpdate as separate top-level args for Comlink serialization
    await this.#raceDisposal(
      (this.remote.dev as any)(
        options?.config,
        options?.onUpdate
          ? comlink.proxy((result: BuildOutput) => {
              handleIssues(result.issues, false, false);
              options.onUpdate!(result);
            })
          : undefined,
      ),
    );
  }

  public async hmrSubscribe(
    identifier: string,
    callback: (update: unknown) => void,
  ): Promise<void> {
    await this.#call(() =>
      Promise.resolve(
        this.remote.hmrSubscribe(identifier, comlink.proxy(callback)),
      ),
    );
  }

  public updateInfoSubscribe(
    aggregationMs: number,
    callback: (message: UpdateMessage) => void,
  ): void {
    this.#throwIfDisposed();
    this.remote.updateInfoSubscribe(aggregationMs, comlink.proxy(callback));
  }

  public async readFile(path: string, encoding?: "utf8") {
    return (await this.#call(() =>
      this.remote.readFile(path, encoding),
    )) as any;
  }

  public async writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ) {
    if (content instanceof Uint8Array) {
      return await this.#call(() =>
        this.remote.writeFile(path, content, encoding),
      );
    }
    return await this.#call(() =>
      this.remote.writeFile(path, content, encoding),
    );
  }

  public async copyFile(src: string, dst: string) {
    return await this.#call(() => this.remote.copyFile(src, dst));
  }

  public async readdir(
    path: string,
    options?: { recursive?: boolean },
  ): Promise<Dirent[]> {
    const dirEntry = (await this.#call(() =>
      this.remote.readdir(path, options),
    )) as any as RawDirent[];
    return dirEntry.map((e) => new Dirent(e));
  }

  public async mkdir(path: string, options?: { recursive?: boolean }) {
    return await this.#call(() => this.remote.mkdir(path, options));
  }

  public async rm(path: string, options?: { recursive?: boolean }) {
    return await this.#call(() => this.remote.rm(path, options));
  }

  public async rmdir(path: string, options?: { recursive?: boolean }) {
    return await this.#call(() => this.remote.rmdir(path, options));
  }

  public async stat(path: string): Promise<Stats> {
    const raw = (await this.#call(() =>
      this.remote.stat(path),
    )) as any as RawStats;
    return new Stats(raw);
  }

  public async gzip(files: PackFile[]): Promise<Uint8Array> {
    return await this.#call(() => this.remote.gzip(files));
  }

  public async sigMd5(content: Uint8Array): Promise<string> {
    return await this.#call(() => this.remote.sigMd5(content));
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
    this.#throwIfDisposed();
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
