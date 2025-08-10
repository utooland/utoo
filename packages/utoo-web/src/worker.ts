import * as comlink from "comlink";
import { HandShake } from "./message";
import { ProjectEndpoint } from "./type";
import initWasm, { DirEntry, Project as ProjectInternal } from "./utoo";

const projectEndpoint: ProjectEndpoint & {
  projectInternal?: ProjectInternal;
  mount: (cwd: string) => Promise<void>;
} = {
  projectInternal: undefined,

  async mount(cwd: string): Promise<void> {
    await initWasm();
    this.projectInternal = new ProjectInternal(cwd);
  },

  async install(packageLock: string): Promise<void> {
    await this.projectInternal!.install(packageLock);
  },

  async build(): Promise<void> {
    await this.projectInternal!.build();
  },

  async readFile(path: string): Promise<string> {
    return await this.projectInternal!.readFile(path);
  },

  async writeFile(path: string, content: string): Promise<void> {
    return await this.projectInternal!.writeFile(path, content);
  },

  async copyFile(src: string, dst: string): Promise<void> {
    return await this.projectInternal!.copyFile(src, dst);
  },

  async readDir(path: string): Promise<DirEntry[]> {
    return await this.projectInternal!.readDir(path);
  },

  async createDir(path: string): Promise<void> {
    return await this.projectInternal!.createDir(path);
  },

  async createDirAll(path: string): Promise<void> {
    return await this.projectInternal!.createDirAll(path);
  },
};

const ConnectedPorts = new Set<MessagePort>();

self.addEventListener("message", (e) => {
  const port = e.ports[0];
  if (e.data === HandShake && !ConnectedPorts.has(port)) {
    comlink.expose(projectEndpoint, port);
    ConnectedPorts.add(port);
  }
});
