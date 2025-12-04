import { cjs } from "./cjs";
import  initWasm, {recvMessageInWorker, recvWorkerRequest,sendTaskMessage, notifyWorkerAck } from "../../utoo"
import { LoaderRunnerMeta } from "./type";

const binding =  {
    recvWorkerRequest,
    recvMessageInWorker,
    notifyWorkerAck,
    sendTaskMessage
  };


declare let self: DedicatedWorkerGlobalScope & {
  process: {
      env: Record<string, string>
      cwd: () => string
    }
  workerData: {
    workerId: number
    poolId: string
    cwd: string,
    binding: typeof binding
    readFile(path: string, encoding?: 'utf8'): Promise<string>
  }
}

self.process = {
    env: {},
    cwd: () => self.workerData.cwd
  }

self.onmessage = async (event) => {
  let [module, memory, meta] =
    event.data as [WebAssembly.Module, WebAssembly.Memory, LoaderRunnerMeta]


   await initWasm(module, memory).catch((err: Error) => {
    console.log(err);
    throw err
  })

  self.workerData = {
    poolId: meta.workerData.poolId,
    workerId: meta.workerData.workerId,
    cwd: "./",
    binding,
    readFile: async (path: string) => {
      // TODO: if we want that, just connect to @utoo/web internalProject endpoint port with comlink
       throw new Error('readFile in loader not supported on browser ')
    }
  }


  cjs(meta.loaderAssets.entrypoint, meta.loaderAssets.importMaps)
}

export default null;
