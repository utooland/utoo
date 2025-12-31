import initWasm, {
  Project,
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
    fs?: Project;
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

    // Initialize the thread-local state (tokio runtime).
    // We don't need to pass threadWorkerUrl here because it's already stored in a global static in Rust.
    Project.init("");
    Project.setCwd(meta.workerData.cwd);

    self.workerData = {
      threadId: meta.workerData.threadId,
      cwd: meta.workerData.cwd,
      projectRoot: meta.workerData.projectRoot,
      binding,
      fs: Project as any,
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
