import { SabComClient } from "../../sabcom";
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
    cwd: string;
    binding: typeof binding;
    sabClient?: SabComClient;
  };
};

export function startLoaderWorker() {
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

    const sabClient = meta.sab
      ? new SabComClient(meta.sab, () => {
          self.postMessage("sab_request");
        })
      : undefined;

    self.workerData = {
      workerId: meta.workerData.workerId,
      cwd: meta.workerData.cwd,
      binding,
      sabClient,
    };

    self.process = {
      env: {},
      cwd: () => self.workerData.cwd,
    };
    console.log("Worker CWD:", self.process.cwd());

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
