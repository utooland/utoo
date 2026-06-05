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

const ENTRYPOINT_READ_RETRY_DELAYS_MS = [10, 20, 40, 80, 160, 250];

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const getErrorMessage = (error: unknown) => {
  if (error && typeof error === "object") {
    const { code, message } = error as { code?: string; message?: string };
    return [code, message].filter(Boolean).join(": ");
  }
  return String(error);
};

const isMissingEntrypointError = (error: unknown) => {
  if (error && typeof error === "object") {
    const { code, message } = error as { code?: string; message?: string };
    if (code === "ENOENT") return true;
    return Boolean(
      message?.includes("NotFoundError") || message?.includes("ENOENT"),
    );
  }
  return String(error).includes("NotFoundError");
};

const waitForEntrypointReadable = async (entrypoint: string) => {
  let lastError: unknown;

  for (let attempt = 0; ; attempt += 1) {
    try {
      Fs.readSync(entrypoint);
      return;
    } catch (error) {
      lastError = error;
      if (
        !isMissingEntrypointError(error) ||
        attempt >= ENTRYPOINT_READ_RETRY_DELAYS_MS.length
      ) {
        break;
      }
      await delay(ENTRYPOINT_READ_RETRY_DELAYS_MS[attempt]);
    }
  }

  if (lastError && isMissingEntrypointError(lastError)) {
    console.warn("Worker: entrypoint was not readable before cjs load", {
      entrypoint,
      attempts: ENTRYPOINT_READ_RETRY_DELAYS_MS.length + 1,
      error: getErrorMessage(lastError),
    });
  }
};

const handleLoaderWorkerMessage = async (event: MessageEvent) => {
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

  await waitForEntrypointReadable(meta.loaderAssets.entrypoint);
  await cjs(meta.loaderAssets.entrypoint, meta.loaderAssets.importMaps);
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
    void handleLoaderWorkerMessage(event).catch((error) => {
      setTimeout(() => {
        throw error;
      });
    });
  };
}

// @ts-ignore
if (typeof __webpack_require__ !== "undefined") {
  // @ts-ignore
  self.startLoaderWorker = startLoaderWorker;
}

startLoaderWorker();
export default startLoaderWorker;
