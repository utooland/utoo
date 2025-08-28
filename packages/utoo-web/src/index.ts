import * as comlink from "comlink";
import { Fork, HandShake, ServiceWorkerHandShake } from "./message";
import { Dirent, ProjectEndpoint, ProjectOptions, RawDirent } from "./type";

let ProjectWorker: Worker;

const ConnectedPorts = new Set<MessagePort>();

export class Project implements ProjectEndpoint {
  #tunnel: Promise<void>;

  private serviceWorkerUrl: string;

  private proxiedResourcePath: string;

  private remote: comlink.Remote<
    ProjectEndpoint & {
      mount: (
        opt: Omit<
          ProjectOptions,
          "workerUrl" | "serviceWorkerUrl" | "proxiedResourcePath"
        >,
      ) => Promise<void>;
    }
  >;

  constructor(options: ProjectOptions) {
    const {
      cwd,
      workerUrl,
      wasmUrl,
      threadWorkerUrl,
      serviceWorkerUrl,
      proxiedResourcePath,
    } = options;

    this.serviceWorkerUrl = serviceWorkerUrl;

    this.proxiedResourcePath = proxiedResourcePath;

    const { port1, port2 } = new MessageChannel();

    this.remote ??= comlink.wrap(port1);

    if (!ProjectWorker) {
      ProjectWorker = workerUrl
        ? new Worker(workerUrl)
        : new Worker(new URL("./worker", import.meta.url));
      window.addEventListener("message", (e) => {
        this.connectWorker(e);
      });
      navigator.serviceWorker.addEventListener("message", (e) => {
        this.connectWorker(e);
      });
    }

    ProjectWorker.postMessage(HandShake, [port2]);

    this.#tunnel ??= this.remote.mount({
      cwd,
      wasmUrl,
      threadWorkerUrl,
    });
  }

  private connectWorker(e: MessageEvent) {
    const port = e.ports[0];
    if (e.data === Fork && !ConnectedPorts.has(port)) {
      ProjectWorker.postMessage(HandShake, [port]);
    }
  }

  public async installServiceWorker() {
    await navigator.serviceWorker.register(this.serviceWorkerUrl);

    navigator.serviceWorker.controller?.postMessage({
      [ServiceWorkerHandShake]: true,
      previewPath: this.proxiedResourcePath,
    });
  }

  public async install(packageLock: string): Promise<void> {
    await this.#tunnel;
    return await this.remote.install(packageLock);
  }

  public async build(): Promise<void> {
    await this.#tunnel;
    return await this.remote.build();
  }

  public async readFile(path: string, encoding?: "utf8") {
    await this.#tunnel;
    return (await this.remote.readFile(path, encoding)) as any;
  }

  public async writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ) {
    await this.#tunnel;
    return await this.remote.writeFile(path, content, encoding);
  }

  public async copyFile(src: string, dst: string) {
    await this.#tunnel;
    return await this.remote.copyFile(src, dst);
  }

  public async readdir(
    path: string,
    options?: { recursive?: boolean },
  ): Promise<Dirent[]> {
    await this.#tunnel;
    const dirEntry = (await this.remote.readdir(
      path,
      options,
    )) as any as RawDirent[];
    return dirEntry.map((e) => new Dirent(e));
  }

  public async mkdir(
    path: string,
    options?: { recursive?: boolean },
  ): Promise<void> {
    await this.#tunnel;
    return await this.remote.mkdir(path, options);
  }

  public static fork(
    channel: MessageChannel,
    eventSource: Client | DedicatedWorkerGlobalScope,
  ): ProjectEndpoint {
    eventSource.postMessage(Fork, {
      transfer: [channel.port2],
    });

    return new ForkedProject(channel.port1);
  }
}

class ForkedProject implements ProjectEndpoint {
  private endpoint: comlink.Remote<ProjectEndpoint>;

  constructor(port: MessagePort) {
    this.endpoint ??= comlink.wrap(port);
  }

  public async install(packageLock: string) {
    return await this.endpoint.install(packageLock);
  }

  public async build() {
    return await this.endpoint.build();
  }

  public async readFile(path: string, encoding?: "utf8") {
    return (await this.endpoint.readFile(path, encoding)) as any;
  }

  public async writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ) {
    return await this.endpoint.writeFile(path, content, encoding);
  }

  public async copyFile(src: string, dst: string): Promise<void> {
    return await this.endpoint.copyFile(src, dst);
  }

  public async readdir(path: string, options?: { recursive?: boolean }) {
    return await this.endpoint.readdir(path, options);
  }

  public async mkdir(path: string, options?: { recursive?: boolean }) {
    return await this.endpoint.mkdir(path, options);
  }
}

export * from "./type";
