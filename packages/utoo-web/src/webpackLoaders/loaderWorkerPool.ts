import * as comlink from "comlink";
import { internalEndpoint } from "../internalProject";
import * as sabcom from "../sabcom";
import { Binding } from "../type";
import initWasm, {
  Project as ProjectInternal,
  registerWorkerScheduler,
  WebWorkerCreation,
  WebWorkerTermination,
  workerCreated,
} from "../utoo";
import { LoaderRunnerMeta } from "./type";

let nextWorkerId = 0;

const loaderWorkers: Record<string, Map<number, Worker>> = {};

export const runLoaderWorkerPool = async (
  binding: Binding,
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

      const sab = new SharedArrayBuffer(1024 * 1024 * 10); // 10MB
      const sabHost = new sabcom.SabComHost(sab);

      const worker = new Worker(loaderWorkerUrl, { name: filename });
      worker.onmessage = async (event) => {
        if (event.data === "sab_request") {
          await sabcom.handleSabRequest(sabHost, {
            read: (path) => ProjectInternal.read(path),
            readDir: (path) => ProjectInternal.readDir(path),
            writeString: (path, content) =>
              ProjectInternal.writeString(path, content),
            createDirAll: (path) => ProjectInternal.createDirAll(path),
            createDir: (path) => ProjectInternal.createDir(path),
            metadata: (path) => ProjectInternal.metadata(path),
            removeFile: (path) => ProjectInternal.removeFile(path),
            removeDir: (path, recursive) =>
              ProjectInternal.removeDir(path, recursive),
            copyFile: (src, dst) => ProjectInternal.copyFile(src, dst),
          });
        }
      };

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
        // @ts-ignore
        initWasm.__wbindgen_wasm_module,
        binding.memory,
        {
          workerData: {
            cwd: finalCwd,
            projectRoot: projectCwd,
            workerId: workerId,
          },
          loaderAssets: {
            importMaps: { ...loadersImportMap },
            entrypoint: finalFilename,
          },
          sab,
        } as LoaderRunnerMeta,
      ]);
      const workers =
        loaderWorkers[filename] || (loaderWorkers[filename] = new Map());
      workers.set(workerId, worker);

      workerCreated(workerId);
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
