import {
  Binding,
  PackFile,
  ProjectEndpoint,
  ProjectOptions,
  RawDirent,
} from "./type";
import initWasm, {
  DirEntryType,
  init_log_filter,
  Project as ProjectInternal,
} from "./utoo";
import { LoaderRunnerMeta } from "./webpackLoaders/worker/type";
// @ts-ignore
import webpackLoadersCode from "./webpackLoaders/workerContent";

let nextWorkerId = 0;

const loaderWorkers: Record<string, Array<Worker & { workerId: number }>> = {};

async function getTurbopackLoaderAssets() {
  const turbopackLoaderAssets: Record<string, string> = {};
  const dirEntries =
    await internalEndpoint.projectInternal!.readDir(".turbopack");
  await Promise.all(
    dirEntries.map(async (entry) => {
      // only collect .js files
      if (entry.type === "file" && entry.name.endsWith(".js")) {
        turbopackLoaderAssets[`./${entry.name}`] =
          await internalEndpoint.projectInternal!.readToString(
            `.turbopack/${entry.name}`,
          );
      }
    }),
  );
  return turbopackLoaderAssets;
}

const createOrScalePool = async (binding: Binding, cwd: string) => {
  while (true) {
    try {
      let poolOptions = await binding.recvPoolRequest();
      const { filename: entrypoint, maxConcurrency } = poolOptions;
      const workers =
        loaderWorkers[entrypoint] || (loaderWorkers[entrypoint] = []);
      if (workers.length < maxConcurrency) {
        for (let i = workers.length; i < maxConcurrency; i++) {
          nextWorkerId += 1;
          const turbopackLoaderAssets: Record<string, string> =
            await getTurbopackLoaderAssets();

          const blob = new Blob([webpackLoadersCode], {
            type: "text/javascript",
          });

          const workerUrl = URL.createObjectURL(blob);
          const worker = new Worker(workerUrl);
          worker.postMessage([
            // @ts-ignore
            initWasm.__wbindgen_wasm_module,
            binding.memory,
            {
              workerData: {
                poolId: entrypoint,
                workerId: nextWorkerId,
              },
              loaderAssets: {
                importMaps: turbopackLoaderAssets,
                entrypoint: entrypoint.replace(".turbopack/", ""),
              },
            } as LoaderRunnerMeta,
          ]);
          // @ts-ignore
          worker.workerId = nextWorkerId;
          // @ts-ignore
          workers.push(worker);
        }
      } else if (workers.length > maxConcurrency) {
        const workersToStop = workers.splice(
          0,
          workers.length - maxConcurrency,
        );
        workersToStop.forEach((worker) => worker.terminate());
      }
    } catch (_e) {
      // rust channel closed. do nothing
      return;
    }
  }
};

const waitingForWorkerTermination = async (binding: Binding) => {
  while (true) {
    try {
      const { filename, workerId } = await binding.recvWorkerTermination();
      const workers = loaderWorkers[filename];
      const workerIdx = workers.findIndex(
        (worker) => worker.workerId === workerId,
      );
      if (workerIdx > -1) {
        const worker = workers.splice(workerIdx, 1);
        worker[0].terminate();
      }
    } catch (_e) {
      // rust channel closed. do nothing
      return;
    }
  }
};

const internalEndpoint: ProjectEndpoint & {
  projectInternal?: ProjectInternal;
  mount: (
    opt: Omit<ProjectOptions, "workerUrl" | "serviceWorker">,
  ) => Promise<void>;
  wasmInit?: ReturnType<typeof initWasm>;
} = {
  projectInternal: undefined,
  wasmInit: undefined,

  // This should be called only once
  async mount(opt) {
    const { cwd, wasmUrl, threadWorkerUrl, logFilter } = opt;

    this.wasmInit ??= initWasm(wasmUrl);
    await this.wasmInit!;

    // Initialize log filter after wasm init
    const filter = logFilter || "pack_core=info,pack_api=info,utoo_wasm=info";
    init_log_filter(filter);

    this.projectInternal = new ProjectInternal(cwd, threadWorkerUrl);
    return;
  },

  async install(packageLock: string, maxConcurrentDownloads?: number) {
    await this.wasmInit!;
    await this.projectInternal!.install(packageLock, maxConcurrentDownloads);
    return;
  },

  async build() {
    const binding = await this.wasmInit!;

    createOrScalePool(binding, this.projectInternal!.cwd);
    waitingForWorkerTermination(binding);

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

  async gzip(files: PackFile[]) {
    await this.wasmInit!;
    return await this.projectInternal!.gzip(files);
  },

  async sigMd5(content: Uint8Array) {
    await this.wasmInit!;
    return await this.projectInternal!.sigMd5(content);
  },
};

export { internalEndpoint };
