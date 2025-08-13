import * as comlink from "comlink";
import { HandShake } from "./message";
import { ProjectEndpoint, RawDirent } from "./type";
import initWasm, { DirEntryType, Project as ProjectInternal } from "./utoo";

const projectEndpoint: ProjectEndpoint & {
  projectInternal?: ProjectInternal;
  mount: (cwd: string) => Promise<void>;
} = {
  projectInternal: undefined,

  async mount(cwd: string) {
    await initWasm();
    this.projectInternal = new ProjectInternal(cwd);
    return;
  },

  async install(packageLock: string) {
    await this.projectInternal!.install(packageLock);
    return;
  },

  async build() {
    return await this.projectInternal!.build();
  },

  async readFile(path: string, encoding?: "utf8") {
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
    encoding?: "utf8",
  ) {
    if (typeof content === "string") {
      if (encoding !== "utf8") {
        throw new Error("Invalid encoding");
      }
      return await this.projectInternal!.writeString(path, content);
    } else {
      return await this.projectInternal!.write(path, content);
    }
  },

  async copyFile(src: string, dst: string) {
    return await this.projectInternal!.copyFile(src, dst);
  },

  async readdir(path: string, options?: { recursive?: boolean }) {
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
