import * as comlink from "comlink";
import { Fork, HandShake } from "./message";
import { ProjectEndpoint, DirEntry } from "./type";

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

  public async readFile(path: string): Promise<string> {
    await this.#tunnel;
    return await this.remote.readFile(path);
  }

  public async writeFile(path: string, content: string): Promise<void> {
    await this.#tunnel;
    return await this.remote.writeFile(path, content);
  }

  public async copyFile(src: string, dst: string): Promise<void> {
    await this.#tunnel;
    return await this.remote.copyFile(src, dst);
  }

  public async readDir(path: string): Promise<DirEntry[]> {
    await this.#tunnel;
    return await this.remote.readDir(path);
  }

  public async createDir(path: string): Promise<void> {
    await this.#tunnel;
    return await this.remote.createDir(path);
  }

  public async createDirAll(path: string): Promise<void> {
    await this.#tunnel;
    return await this.remote.createDirAll(path);
  }

  // This should be called from different worker
  public static fork(port1: MessagePort, port2: MessagePort): ProjectEndpoint {
    (self as any as MessagePort).postMessage(Fork, [port2]);

    return new ForkedProject(port1);
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

  public async readFile(path: string): Promise<string> {
    return await this.endpoint.readFile(path);
  }

  public async writeFile(path: string, content: string): Promise<void> {
    return await this.endpoint.writeFile(path, content);
  }

  public async copyFile(src: string, dst: string): Promise<void> {
    return await this.endpoint.copyFile(src, dst);
  }

  public async readDir(path: string): Promise<DirEntry[]> {
    return await this.endpoint.readDir(path);
  }

  public async createDir(path: string): Promise<void> {
    return await this.endpoint.createDir(path);
  }

  public async createDirAll(path: string): Promise<void> {
    return await this.endpoint.createDirAll(path);
  }
}

export * from "./type";
