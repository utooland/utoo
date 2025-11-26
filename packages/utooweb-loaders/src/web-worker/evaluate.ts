import { threadId as workerId, workerData } from 'worker_threads'
import { structuredError } from '../error'
import type { Channel } from '../types'

import initWasm from "@utoo/web/esm/utoo"

interface Binding {
  recvWorkerRequest(poolId: string): Promise<number>
  recvMessageInWorker(workerId: number): Promise<string>
  notifyOneWorkerCreated(filename: string): Promise<void>
  notifyWorkerAck(taskId: number, workerId: number): Promise<void>
  sendTaskMessage(taskId: number, message: string): Promise<void>
}

let binding: Binding;

self.onmessage = async event => {
  let [module, memory] = event.data;

  if (binding) {
    return;
  }

  binding = await initWasm(module, memory).catch((err: Error) => {
    console.log(err);

    // Propagate to main `onerror`:
    setTimeout(() => {
      throw err;
    });

    // Rethrow to keep promise rejected and prevent execution of further commands:
    throw err;
  }).then((wasm: Binding) => {
    wasm.notifyOneWorkerCreated(workerData.poolId)
    return wasm
  });
};



const queue: string[][] = []

export const run = async (
  moduleFactory: () => Promise<{
    init?: () => Promise<void>
    default: (channel: Channel<any, any>, ...deserializedArgs: any[]) => any
  }>
) => {
  const taskId = await binding.recvWorkerRequest(workerData.poolId)

  await binding.notifyWorkerAck(taskId, workerId)

  let nextId = 1
  const requests = new Map()
  const internalIpc = {
    sendInfo: (message: any) =>
      binding.sendTaskMessage(
        taskId,
        JSON.stringify({
          type: 'info',
          data: message,
        })
      ),
    sendRequest: async (message: any) => {
      const id = nextId++
      let resolve, reject
      const promise = new Promise((res, rej) => {
        resolve = res
        reject = rej
      })
      requests.set(id, { resolve, reject })
      return binding
        .sendTaskMessage(
          taskId,
          JSON.stringify({ type: 'request', id, data: message })
        )
        .then(() => promise)
    },
    sendError: async (error: Error) => {
      try {
        await binding.sendTaskMessage(
          taskId,
          JSON.stringify({
            type: 'error',
            ...structuredError(error),
          })
        )
      } catch (err) {
        // There's nothing we can do about errors that happen after this point, we can't tell anyone
        // about them.
        console.error('failed to send error back to rust:', err)
      }
    },
  }

  let getValue: (channel: Channel<any, any>, ...deserializedArgs: any[]) => any
  try {
    const module = await moduleFactory()
    if (typeof module.init === 'function') {
      await module.init()
    }
    getValue = module.default
  } catch (err) {
    try {
      await binding.sendTaskMessage(
        taskId,
        JSON.stringify({
          type: 'error',
          ...structuredError(err as Error),
        })
      )
    } catch (err) {
      // There's nothing we can do about errors that happen after this point, we can't tell anyone
      // about them.
      console.error('failed to send error back to rust:', err)
    }
  }

  let isRunning = false

  const run = async () => {
    while (queue.length > 0) {
      const args = queue.shift()!
      try {
        const value = await getValue(internalIpc, ...args)
        await binding.sendTaskMessage(
          taskId,
          JSON.stringify({
            type: 'end',
            data: value === undefined ? undefined : JSON.stringify(value),
            duration: 0,
          })
        )
      } catch (err) {
        await binding.sendTaskMessage(
          taskId,
          JSON.stringify({
            type: 'error',
            ...structuredError(err as Error),
          })
        )
      }
    }
    isRunning = false
  }

  while (true) {
    const msg_str = await binding.recvMessageInWorker(workerId)

    const msg = JSON.parse(msg_str) as
      | {
        type: 'evaluate'
        args: string[]
      }
      | {
        type: 'result'
        id: number
        error?: string
        data?: any
      }

    switch (msg.type) {
      case 'evaluate': {
        queue.push(msg.args)
        if (!isRunning) {
          isRunning = true
          run()
        }
        break
      }
      case 'result': {
        const request = requests.get(msg.id)
        if (request) {
          requests.delete(msg.id)
          if (msg.error) {
            request.reject(new Error(msg.error))
          } else {
            request.resolve(msg.data)
          }
        }
        break
      }
      default: {
        console.error('unexpected message type', (msg as any).type)
      }
    }
  }
}
