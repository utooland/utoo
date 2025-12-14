import { Binding } from "./type";
import initWasm, {
  Project as ProjectInternal,
  WebWorkerCreation,
  WebWorkerTermination,
  workerCreated,
} from "./utoo";
import { LoaderRunnerMeta } from "./webpackLoaders/worker/type";
// @ts-ignore
import webpackLoadersCode from "./webpackLoaders/workerContent";

let nextWorkerId = 0;

const loaderWorkers: Record<string, Map<number, Worker>> = {};

async function getTurbopackLoaderAssets(projectInternal: ProjectInternal) {
  const turbopackLoaderAssets: Record<string, string> = {};
  const dirEntries = await projectInternal.readDir(".turbopack");
  await Promise.all(
    dirEntries.map(async (entry) => {
      if (entry.type === "file" && entry.name.endsWith(".js")) {
        turbopackLoaderAssets[`./${entry.name}`] =
          await projectInternal.readToString(`.turbopack/${entry.name}`);
      }
    }),
  );
  return turbopackLoaderAssets;
}

export const runLoaderWorkerPool = async (
  binding: Binding,
  projectInternal: ProjectInternal,
  loadersImportMap?: Record<string, string>,
) => {
  binding.registerWorkerScheduler(
    async (creation: WebWorkerCreation) => {
      const { options } = creation;
      const entrypoint = options.filename;
      nextWorkerId += 1;
      const workerId = nextWorkerId;

      const turbopackLoaderAssets: Record<string, string> =
        await getTurbopackLoaderAssets(projectInternal);

      const blob = new Blob([webpackLoadersCode], {
        type: "text/javascript",
      });

      const workerUrl = URL.createObjectURL(blob);
      const worker = new Worker(workerUrl, { name: entrypoint });
      worker.postMessage([
        // @ts-ignore
        initWasm.__wbindgen_wasm_module,
        binding.memory,
        {
          workerData: {
            poolId: entrypoint,
            workerId: workerId,
          },
          loaderAssets: {
            importMaps: { ...turbopackLoaderAssets, ...loadersImportMap },
            entrypoint: entrypoint.replace(".turbopack/", ""),
          },
        } as LoaderRunnerMeta,
      ]);
      const workers =
        loaderWorkers[entrypoint] || (loaderWorkers[entrypoint] = new Map());
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
