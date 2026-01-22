import {
  Fs,
  initSync,
  recvTaskMessageInWorker,
  sendTaskMessage,
  workerCreated,
} from "../utoo";
import { cjs } from "./cjs";
import { LoaderRunnerMeta } from "./types";

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
    threadId: number;
    cwd: string;
    projectRoot: string;
    binding: typeof binding;
    fs?: typeof Fs;
  };
};

export function startLoaderWorker() {
  self.onmessage = (event) => {
    let [module, memory, meta] = event.data as [
      WebAssembly.Module,
      WebAssembly.Memory,
      LoaderRunnerMeta,
    ];

    try {
      initSync({ module, memory });
    } catch (err) {
      console.error(err);
      throw err;
    }

    self.workerData = {
      threadId: meta.workerData.threadId,
      cwd: meta.workerData.cwd,
      projectRoot: meta.workerData.projectRoot,
      binding,
      fs: Fs,
    };

    self.process = {
      env: {},
      cwd: () => self.workerData.cwd,
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
