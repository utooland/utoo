import * as sabcom from "./sabcom";
import { Binding } from "./type";
import initWasm, {
  Project as ProjectInternal,
  registerWorkerScheduler,
  WebWorkerCreation,
  WebWorkerTermination,
  workerCreated,
} from "./utoo";
import { LoaderRunnerMeta } from "./webpackLoaders/worker/type";

let nextWorkerId = 0;

const loaderWorkers: Record<string, Map<number, Worker>> = {};

export const runLoaderWorkerPool = async (
  binding: Binding,
  projectCwd: string,
  projectInternal: ProjectInternal,
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
            read: (path) => projectInternal.read(path),
            readDir: (path) => projectInternal.readDir(path),
            writeString: (path, content) =>
              projectInternal.writeString(path, content),
            createDirAll: (path) => projectInternal.createDirAll(path),
            createDir: (path) => projectInternal.createDir(path),
            metadata: (path) => projectInternal.metadata(path),
            removeFile: (path) => projectInternal.removeFile(path),
            removeDir: (path, recursive) =>
              projectInternal.removeDir(path, recursive),
            copyFile: (src, dst) => projectInternal.copyFile(src, dst),
          });
        }
      };

      let finalCwd = cwd;
      let finalFilename = filename;

      if (projectCwd) {
        const sep = "/";
        const pCwd = projectCwd.endsWith(sep)
          ? projectCwd.slice(0, -1)
          : projectCwd;

        let cCwd = cwd.startsWith(sep) ? cwd.slice(1) : cwd;
        if (cCwd === "." || cCwd === "./") {
          cCwd = "";
        }
        finalCwd = cCwd ? `${pCwd}${sep}${cCwd}` : pCwd;

        if (!filename.startsWith("/")) {
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
