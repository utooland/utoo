import * as comlink from "comlink";
import { HandShake } from "./message";
import { PackFile, ProjectEndpoint, ProjectOptions, RawDirent } from "./type";
import initWasm, { DirEntryType, Project as ProjectInternal } from "./utoo";

declare let self: DedicatedWorkerGlobalScope;

const projectEndpoint: ProjectEndpoint & {
  projectInternal?: ProjectInternal;
  mount: (
    opt: Omit<ProjectOptions, "workerUrl" | "serviceWorker">,
  ) => Promise<void>;
  wasmInit?: ReturnType<typeof initWasm>;
} = {
  projectInternal: undefined,
  wasmInit: undefined,

  get cwd(): string {
    return this.projectInternal?.cwd || "";
  },

  // This should be called only once
  async mount(opt) {
    const { cwd, wasmUrl, threadWorkerUrl, logFilter } = opt;

    // Set global log filter before wasm init
    if (logFilter) {
      (globalThis as any).__UTOO_LOG_FILTER__ = logFilter;
    }

    this.wasmInit ??= initWasm(wasmUrl);
    await this.wasmInit!;

    // Pass logFilter to thread worker via URL query string
    let finalThreadWorkerUrl = threadWorkerUrl;
    if (logFilter) {
      const url = new URL(threadWorkerUrl, self.location.href);
      url.searchParams.set("logFilter", logFilter);
      finalThreadWorkerUrl = url.toString();
    }

    this.projectInternal = new ProjectInternal(cwd, finalThreadWorkerUrl);
    return;
  },

  async install(packageLock: string, maxConcurrentDownloads?: number) {
    await this.wasmInit!;
    await this.projectInternal!.install(packageLock, maxConcurrentDownloads);
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

  async rm(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    let metadata = (await this.projectInternal!.metadata(path)).toJSON();

    switch ((metadata as any).type as DirEntryType) {
      case "file":
        return await this.projectInternal!.removeFile(path);
      case "directory":
        return await this.projectInternal!.removeDir(
          path,
          !!options?.recursive,
        );
      default:
        // nothing to remove now
        break;
    }
  },

  async rmdir(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    return await this.projectInternal!.removeDir(path, !!options?.recursive);
  },

  async sigMd5(content: Uint8Array): Promise<string> {
    await this.wasmInit!;
    return this.projectInternal!.sigMd5(content);
  },

  async gzip(files: PackFile[], dest: string) {
    await this.wasmInit!;
    return await this.projectInternal!.gzip(files, dest);
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
