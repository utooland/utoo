import { type ConfigComplete, type UpdateMessage } from "@utoo/pack-shared";
import {
  DepsOptions,
  InstallOptions,
  PackFile,
  ProjectEndpoint,
  ProjectOptions,
  RawDirent,
  RawStats,
  Stats,
} from "../types";
import initWasm, {
  BuildOptions,
  DirEntryType,
  Fs,
  initLogFilter,
  Project as ProjectInternal,
  RootTask,
} from "../utoo";
import {
  disposeLoaderWorkerPool,
  runLoaderWorkerPool,
} from "../webpackLoaders/loaderWorkerPool";

class InternalEndpoint implements ProjectEndpoint {
  wasmInit?: ReturnType<typeof initWasm>;
  options?: Omit<ProjectOptions, "workerUrl" | "serviceWorker">;
  loaderWorkerPoolInitialized = false;

  // Keep root task alive for the subscription to work
  private rootTask?: RootTask;
  // Keep HMR root tasks alive (keyed by identifier)
  private hmrRootTasks: Map<string, RootTask> = new Map();

  // This should be called only once
  async mount(opt: Omit<ProjectOptions, "workerUrl" | "serviceWorker">) {
    this.options = opt;
    const { cwd, wasmUrl, threadWorkerUrl, logFilter } = opt;

    this.wasmInit ??= initWasm(wasmUrl);
    await this.wasmInit!;

    // Initialize log filter after wasm init
    const filter =
      logFilter ||
      "pack_core=info,pack_api=info,utoo_wasm=info,utoo_ruborist=info";
    initLogFilter(filter);

    const absoluteCwd = cwd.startsWith("/") ? cwd : "/" + cwd;
    ProjectInternal.init(threadWorkerUrl || "");
    ProjectInternal.setCwd(absoluteCwd);
    return;
  }

  async deps(options?: DepsOptions) {
    await this.wasmInit!;
    // Ensure we pass undefined (not null) for missing values
    const registry = options?.registry ?? undefined;
    const concurrency = options?.concurrency ?? undefined;
    return await ProjectInternal.deps(registry, concurrency);
  }

  async install(packageLock: string, options?: InstallOptions) {
    await this.wasmInit!;
    const concurrency = options?.maxConcurrentDownloads ?? undefined;
    await ProjectInternal.install(packageLock, concurrency);
    return;
  }

  async build(options?: { config?: ConfigComplete; cleanup?: boolean }) {
    await this.wasmInit!;

    if (this.options?.loaderWorkerUrl && !this.loaderWorkerPoolInitialized) {
      runLoaderWorkerPool(
        this.options.cwd,
        this.options!.loaderWorkerUrl,
        this.options?.loadersImportMap,
      );
      this.loaderWorkerPoolInitialized = true;
    }

    const buildOptions = new BuildOptions();
    buildOptions.cleanup = options?.cleanup ?? false;
    if (options?.config) {
      buildOptions.config = options.config;
    }
    return await ProjectInternal.build(buildOptions);
  }

  async dispose() {
    this.rootTask?.free();
    this.rootTask = undefined;

    for (const rootTask of this.hmrRootTasks.values()) {
      rootTask.free();
    }
    this.hmrRootTasks.clear();

    disposeLoaderWorkerPool();
    this.loaderWorkerPoolInitialized = false;
    this.options = undefined;

    await ProjectInternal.dispose();
  }

  // @ts-expect-error - Comlink delivers (config, onUpdate) as separate args, not as options object
  async dev(config?: ConfigComplete, onUpdate?: (result: any) => void) {
    if (this.options?.loaderWorkerUrl && !this.loaderWorkerPoolInitialized) {
      runLoaderWorkerPool(
        this.options.cwd,
        this.options!.loaderWorkerUrl,
        this.options?.loadersImportMap,
      );
      this.loaderWorkerPoolInitialized = true;
    }

    this.rootTask = await ProjectInternal.entrypointsSubscribe(
      config,
      (result: any) => {
        onUpdate?.(result);
      },
    );
  }

  async readFile(path: string, encoding?: "utf8") {
    await this.wasmInit!;
    let ret;
    if (encoding === "utf8") {
      ret = await Fs.readToString(path);
    } else {
      return await Fs.read(path);
    }
    return ret as any;
  }

  async writeFile(
    path: string,
    content: string | Uint8Array,
    _encoding?: "utf8",
  ) {
    await this.wasmInit!;
    if (typeof content === "string") {
      return await Fs.writeString(path, content);
    } else {
      return await Fs.write(path, content);
    }
  }

  async copyFile(src: string, dst: string) {
    await this.wasmInit!;
    return await Fs.copyFile(src, dst);
  }

  async stat(path: string): Promise<Stats> {
    await this.wasmInit!;
    const metadata = await Fs.metadata(path);
    const json = metadata.toJSON() as any;
    const raw: RawStats = {
      type: json.type as DirEntryType,
      size: Number(json.file_size),
    };
    // WARN: This is a hack, functions can not be structurally cloned
    return raw as any;
  }

  async readdir(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    const dirEntries = options?.recursive
      ? await Fs.readDir(path)
      : // TODO: support recursive readDirAll
        await Fs.readDir(path);
    const rawDirents: RawDirent[] = dirEntries.map((e: any) => {
      const dir = e.toJSON() as any;
      return {
        name: dir.name as string,
        type: dir.type as DirEntryType,
      };
    });
    // WARN: This is a hack, functions can not be structurally cloned
    return rawDirents as any;
  }

  async mkdir(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    if (options?.recursive) {
      return await Fs.createDirAll(path);
    } else {
      return await Fs.createDir(path);
    }
  }

  async rm(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    let metadata = (await Fs.metadata(path)).toJSON();

    switch ((metadata as any).type as DirEntryType) {
      case "file":
        return await Fs.removeFile(path);
      case "directory":
        return await Fs.removeDir(path, !!options?.recursive);
      default:
        // nothing to remove now
        break;
    }
  }

  async rmdir(path: string, options?: { recursive?: boolean }) {
    await this.wasmInit!;
    return await Fs.removeDir(path, !!options?.recursive);
  }

  async gzip(files: PackFile[]) {
    await this.wasmInit!;
    return await ProjectInternal.gzip(files);
  }

  async sigMd5(content: Uint8Array) {
    await this.wasmInit!;
    return await ProjectInternal.sigMd5(content);
  }

  async hmrSubscribe(identifier: string, callback: (update: unknown) => void) {
    const rootTask = await ProjectInternal.hmrEvents(identifier, callback);
    this.hmrRootTasks.set(identifier, rootTask);
  }

  updateInfoSubscribe(
    aggregationMs: number,
    callback: (message: UpdateMessage) => void,
  ) {
    ProjectInternal.updateInfoSubscribe(aggregationMs, callback);
  }
}

const internalEndpoint = new InternalEndpoint();

export { internalEndpoint };
