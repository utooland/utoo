import {
  getWasmMemory,
  getWasmModule,
  registerWorkerScheduler,
  WebWorkerCreation,
  WebWorkerTermination,
} from "../utoo";
import { createWorkerFromDataUri } from "../workers/inline";
import { LoaderRunnerMeta } from "./types";

let nextWorkerId = 0;

const loaderWorkers: Record<string, Map<number, Worker>> = {};

export const runLoaderWorkerPool = async (
  projectCwd: string,
  loaderWorkerUrl: string,
  loadersImportMap?: Record<string, string>,
) => {
  registerWorkerScheduler(
    async (creation: WebWorkerCreation) => {
      const {
        options: { filename, cwd },
      } = creation;
      nextWorkerId += 1;
      const workerId = nextWorkerId;

      const worker = loaderWorkerUrl.startsWith("data:")
        ? createWorkerFromDataUri(loaderWorkerUrl, { name: filename })
        : new Worker(loaderWorkerUrl, { name: filename });

      let finalCwd = cwd;
      let finalFilename = filename;

      if (projectCwd) {
        const sep = "/";
        let pCwd = projectCwd.endsWith(sep)
          ? projectCwd.slice(0, -1)
          : projectCwd;
        if (!pCwd.startsWith(sep)) {
          pCwd = sep + pCwd;
        }

        if (cwd.startsWith(sep)) {
          finalCwd = cwd;
        } else {
          let cCwd = cwd;
          if (cCwd === "." || cCwd === "./") {
            cCwd = "";
          }
          finalCwd = cCwd ? `${pCwd}${sep}${cCwd}` : pCwd;
        }

        if (filename.startsWith(sep)) {
          finalFilename = filename;
        } else {
          let fName = filename;
          if (fName.startsWith("./")) fName = fName.slice(2);
          finalFilename = `${pCwd}${sep}${fName}`;
        }
      }

      worker.postMessage([
        getWasmModule(),
        getWasmMemory(),
        {
          workerData: {
            cwd: finalCwd,
            projectRoot: projectCwd,
            threadId: workerId,
          },
          loaderAssets: {
            importMaps: loadersImportMap ?? {},
            entrypoint: finalFilename,
          },
        } as LoaderRunnerMeta,
      ]);
      const workers =
        loaderWorkers[filename] || (loaderWorkers[filename] = new Map());
      workers.set(workerId, worker);
    },
    (termination: WebWorkerTermination) => {
      const { workerId, options } = termination;
      const entrypoint = options.filename;
      const workers = loaderWorkers[entrypoint];
      if (workers) {
        const worker = workers.get(workerId);
        if (worker) {
          worker.terminate();
          workers.delete(workerId);
        }
      }
    },
  );
};
