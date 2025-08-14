import * as comlink from "comlink";
import { HandShake } from "./message";
import { ProjectEndpoint, RawDirent } from "./type";
import initWasm, { DirEntryType, Project as ProjectInternal } from "./utoo";

const WasmInit = initWasm();

const projectEndpoint: ProjectEndpoint & {
  projectInternal?: ProjectInternal;
  mount: (cwd: string) => Promise<void>;
} = {
  projectInternal: undefined,

  async mount(cwd: string) {
    await WasmInit;
    this.projectInternal = new ProjectInternal(cwd);
    return;
  },

  async install(packageLock: string) {
    await WasmInit;
    await this.projectInternal!.install(packageLock);
    return;
  },

  async build() {
    await WasmInit;
    return await this.projectInternal!.build();
  },

  async readFile(path: string, encoding?: "utf8") {
    await WasmInit;
    let ret;
    if (encoding === "utf8") {
      ret = await this.projectInternal!.readToString(path);
    } else {
      ret = await this.projectInternal!.read(path);
    }
    return ret as any;
  },

  async writeFile(
    path: string,
    content: string | Uint8Array,
    _encoding?: "utf8",
  ) {
    await WasmInit;
    if (typeof content === "string") {
      return await this.projectInternal!.writeString(path, content);
    } else {
      return await this.projectInternal!.write(path, content);
    }
  },

  async copyFile(src: string, dst: string) {
    await WasmInit;
    return await this.projectInternal!.copyFile(src, dst);
  },

  async readdir(path: string, options?: { recursive?: boolean }) {
    await WasmInit;
    const dirEntries = options?.recursive
      ? await this.projectInternal!.readDir(path)
      : // TODO: support recursive readDirAll
        await this.projectInternal!.readDir(path);
    const newLocal: RawDirent[] = dirEntries.map((e) => {
      const dir = e.toJSON() as any;
      return {
        name: dir.name as string,
        type: dir.type as DirEntryType,
      };
    });
    // WARN: This is a hack, functions can not be structurally cloned
    return newLocal as any;
  },

  async mkdir(path: string, options?: { recursive?: boolean }) {
    await WasmInit;
    if (options?.recursive) {
      return await this.projectInternal!.createDirAll(path);
    } else {
      return await this.projectInternal!.createDir(path);
    }
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
