import {
  SAB_OP_COPY_FILE,
  SAB_OP_MKDIR,
  SAB_OP_READ_DIR,
  SAB_OP_READ_FILE,
  SAB_OP_RM,
  SAB_OP_RMDIR,
  SAB_OP_WRITE_FILE,
  SabComHost,
} from "./sabcom";
import { Binding } from "./type";
import initWasm, {
  Project as ProjectInternal,
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
  workerUrl: string,
  loadersImportMap?: Record<string, string>,
) => {
  binding.registerWorkerScheduler(
    async (creation: WebWorkerCreation) => {
      const {
        options: { filename, cwd },
      } = creation;
      nextWorkerId += 1;
      const workerId = nextWorkerId;

      const sab = new SharedArrayBuffer(1024 * 1024 * 10); // 10MB
      const sabHost = new SabComHost(sab);

      const worker = new Worker(workerUrl, { name: filename });
      worker.onmessage = async (event) => {
        if (event.data === "sab_request") {
          const { op, data: path } = sabHost.readRequest();
          try {
            if (op === SAB_OP_READ_FILE) {
              const bytes = await projectInternal.read(path);
              sabHost.writeResponse(bytes);
            } else if (op === SAB_OP_READ_DIR) {
              const entries = await projectInternal.readDir(path);
              sabHost.writeResponse(
                JSON.stringify(entries.map((e) => e.toJSON())),
              );
            } else if (op === SAB_OP_WRITE_FILE) {
              const { content, encoding } = JSON.parse(path);
              const filePath = content.path;
              const fileContent = content.data;
              // TODO: handle binary content (base64?)
              await projectInternal.writeString(filePath, fileContent);
              sabHost.writeResponse("ok");
            } else if (op === SAB_OP_MKDIR) {
              const { path: dirPath, recursive } = JSON.parse(path);
              if (recursive) {
                await projectInternal.createDirAll(dirPath);
              } else {
                await projectInternal.createDir(dirPath);
              }
              sabHost.writeResponse("ok");
            } else if (op === SAB_OP_RM) {
              const { path: rmPath, recursive } = JSON.parse(path);
              // Mimic internalProject.rm logic
              const metadata = await projectInternal.metadata(rmPath);
              const type = (metadata.toJSON() as any).type;
              if (type === "file") {
                await projectInternal.removeFile(rmPath);
              } else if (type === "directory") {
                await projectInternal.removeDir(rmPath, !!recursive);
              }
              sabHost.writeResponse("ok");
            } else if (op === SAB_OP_RMDIR) {
              const { path: rmPath, recursive } = JSON.parse(path);
              await projectInternal.removeDir(rmPath, !!recursive);
              sabHost.writeResponse("ok");
            } else if (op === SAB_OP_COPY_FILE) {
              const { src, dst } = JSON.parse(path);
              await projectInternal.copyFile(src, dst);
              sabHost.writeResponse("ok");
            } else {
              sabHost.writeError("Unknown op");
            }
          } catch (e: any) {
            sabHost.writeError(e.message);
          }
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
