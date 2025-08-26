import * as comlink from "comlink";
import { HandShake } from "./message";
import { MountOpt, ProjectEndpoint, RawDirent } from "./type";
import initWasm, { DirEntryType, Project as ProjectInternal } from "./utoo";

const projectEndpoint: ProjectEndpoint & {
  projectInternal?: ProjectInternal;
  mount: (opt: MountOpt) => Promise<void>;
  wasmInit?: Promise<any>;
} = {
  projectInternal: undefined,
  wasmInit: undefined,

  // This should be called only once
  async mount(opt) {
    const { cwd, wasmUrl } = opt;
    this.wasmInit ??= initWasm(wasmUrl);
    await this.wasmInit!;
    this.projectInternal = new ProjectInternal(cwd);
    return;
  },

  async install(packageLock: string) {
    await this.wasmInit!;
    await this.projectInternal!.install(packageLock);
    return;
  },

  async build() {
    await this.wasmInit!;
    return await this.projectInternal!.build();
  },

  async readFile(path: string, encoding?: "utf8") {
    await this.wasmInit!;
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
    await this.wasmInit!;
    if (typeof content === "string") {
      return await this.projectInternal!.writeString(path, content);
    } else {
      return await this.projectInternal!.write(path, content);
    }
  },

  async copyFile(src: string, dst: string) {
    await this.wasmInit!;
    return await this.projectInternal!.copyFile(src, dst);
  },

  async readdir(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    const dirEntries = options?.recursive
      ? await this.projectInternal!.readDir(path)
      : // TODO: support recursive readDirAll
        await this.projectInternal!.readDir(path);
    const rawDirents: RawDirent[] = dirEntries.map((e) => {
      const dir = e.toJSON() as any;
      return {
        name: dir.name as string,
        type: dir.type as DirEntryType,
      };
    });
    // WARN: This is a hack, functions can not be structurally cloned
    return rawDirents as any;
  },

  async mkdir(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
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
