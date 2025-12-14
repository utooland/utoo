import initWasm, {
  recvTaskMessageInWorker,
  sendTaskMessage,
  workerCreated,
} from "../../utoo";
import { cjs } from "./cjs";
import { LoaderRunnerMeta } from "./type";

const binding = {
  recvTaskMessageInWorker,
  sendTaskMessage,
  workerCreated,
};

declare let self: DedicatedWorkerGlobalScope & {
  process: {
    env: Record<string, string>;
    cwd: () => string;
  };
  workerData: {
    workerId: number;
    poolId: string;
    cwd: string;
    binding: typeof binding;
    readFile(path: string, encoding?: "utf8"): Promise<string>;
  };
};

export function startLoaderWorker() {
  self.process = {
    env: {},
    cwd: () => self.workerData.cwd,
  };

  self.onmessage = async (event) => {
    let [module, memory, meta] = event.data as [
      WebAssembly.Module,
      WebAssembly.Memory,
      LoaderRunnerMeta,
    ];

    await initWasm(module, memory).catch((err: Error) => {
      console.log(err);
      throw err;
    });

    self.workerData = {
      poolId: meta.workerData.poolId,
      workerId: meta.workerData.workerId,
      cwd: "./",
      binding,
      readFile: async (path: string) => {
        // TODO: if we want that, just connect to @utoo/web internalProject endpoint port with comlink
        throw new Error("readFile in loader not supported on browser ");
      },
    };

    cjs(meta.loaderAssets.entrypoint, meta.loaderAssets.importMaps);
  };
}

// @ts-ignore
if (typeof __webpack_require__ !== "undefined") {
  // @ts-ignore
  self.startLoaderWorker = startLoaderWorker;
}

startLoaderWorker();
export default startLoaderWorker;
