import { handleIssues } from "@utoo/pack-shared";
import * as comlink from "comlink";
import { ForkedProject } from "./forkedProject";
import { installServiceWorker } from "./installServiceWorker";
import { Fork, HandShake } from "./message";
import {
  BuildOutput,
  Dirent,
  PackFile,
  ProjectEndpoint,
  ProjectOptions,
  RawDirent,
  RawStats,
  ServiceWorkerOptions,
  Stats,
} from "./type";

let ProjectWorker: Worker;

const ConnectedPorts = new Set<MessagePort>();

export class Project implements ProjectEndpoint {
  #mount: Promise<void>;

  public readonly cwd: string;

  public readonly serviceWorkerOptions?: ServiceWorkerOptions;

  private remote: comlink.Remote<
    ProjectEndpoint & {
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
      ProjectWorker = new Worker(workerUrl);
      window.addEventListener("message", (e) => {
        this.connectWorker(e);
      });

      if (this.serviceWorkerOptions) {
        navigator.serviceWorker.addEventListener("message", (e) => {
          this.connectWorker(e);
        });
      }
    }

    ProjectWorker.postMessage(HandShake, [port2]);

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
    if (e.data === Fork && !ConnectedPorts.has(port)) {
      ProjectWorker.postMessage(HandShake, [port]);
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

  public async install(packageLock: string, maxConcurrentDownloads?: number) {
    await this.#mount;
    return await this.remote.install(packageLock, maxConcurrentDownloads);
  }

  public async build(): Promise<BuildOutput> {
    await this.#mount;
    const res = await this.remote.build();
    handleIssues(res.issues, false, false);
    return res;
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

  public static fork(
    channel: MessageChannel,
    eventSource?: Client | DedicatedWorkerGlobalScope,
  ): ProjectEndpoint {
    (eventSource || (self as DedicatedWorkerGlobalScope)).postMessage(Fork, {
      transfer: [channel.port2],
    });

    return new ForkedProject(channel.port1);
  }
}
