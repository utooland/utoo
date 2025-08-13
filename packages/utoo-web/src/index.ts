import * as comlink from "comlink";
import { Fork, HandShake } from "./message";
import { Dirent, ProjectEndpoint, RawDirent } from "./type";

let ProjectWorker: Worker;

const ConnectedPorts = new Set<MessagePort>();

export class Project implements ProjectEndpoint {
  private cwd: string;

  #tunnel: Promise<void>;

  private remote: comlink.Remote<
    ProjectEndpoint & { mount: (cwd: string) => Promise<void> }
  >;

  constructor(cwd: string) {
    this.cwd = cwd;

    const { port1, port2 } = new MessageChannel();

    this.remote ??= comlink.wrap(port1);

    if (!ProjectWorker) {
      ProjectWorker = new Worker(new URL("./worker", import.meta.url));

      self.addEventListener("message", (e) => {
        const port = e.ports[0];
        if (e.data === Fork && !ConnectedPorts.has(port)) {
          ProjectWorker.postMessage(HandShake, [port]);
        }
      });
    }

    ProjectWorker.postMessage(HandShake, [port2]);

    this.#tunnel ??= this.remote.mount(this.cwd);
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

  public async writeFile(path: string, content: Uint8Array, encoding?: "utf8") {
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

  public static fork(channel: MessageChannel): ProjectEndpoint {
    self.postMessage(Fork, {
      targetOrigin: "*",
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

  public async writeFile(path: string, content: string, encoding?: "utf8") {
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
