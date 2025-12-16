import { Worker } from "worker_threads";

const loaderWorkers: Record<string, Map<number, Worker>> = {};

function getPoolId(cwd: string, filename: string) {
  return `${cwd}:${filename}`;
}

let workerSchedulerRegistered = false;

export async function runLoaderWorkerPool(
  binding: typeof import("./binding"),
  bindingPath: string,
) {
  if (workerSchedulerRegistered) {
    return;
  }

  binding.registerWorkerScheduler(
    (creation) => {
      const {
        options: { filename, cwd },
      } = creation;

      let poolId = getPoolId(cwd, filename);

      const worker = new Worker(filename, {
        workerData: {
          bindingPath,
          cwd,
        },
      });

      worker.unref();

      const workers =
        loaderWorkers[poolId] || (loaderWorkers[poolId] = new Map());

      workers.set(worker.threadId, worker);
    },
    (termination) => {
      const {
        options: { filename, cwd },
        workerId,
      } = termination;

      let poolId = getPoolId(cwd, filename);

      const workers = loaderWorkers[poolId];

      workers.get(workerId)?.terminate();

      workers.delete(workerId);
    },
  );

  workerSchedulerRegistered = true;
}
