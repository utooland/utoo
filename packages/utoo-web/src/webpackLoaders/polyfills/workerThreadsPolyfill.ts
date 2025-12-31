const globalSelf = self as any;

export const workerData: any = new Proxy(
  {},
  {
    get(_target, prop) {
      return globalSelf.workerData?.[prop];
    },
    has(_target, prop) {
      return globalSelf.workerData && prop in globalSelf.workerData;
    },
    ownKeys(_target) {
      return globalSelf.workerData
        ? Reflect.ownKeys(globalSelf.workerData)
        : [];
    },
    getOwnPropertyDescriptor(_target, prop) {
      return globalSelf.workerData
        ? Object.getOwnPropertyDescriptor(globalSelf.workerData, prop)
        : undefined;
    },
  },
);

export const isMainThread = false;

export const parentPort = {
  postMessage: (message: any) => self.postMessage(message),
  on: (event: string, listener: (...args: any[]) => void) => {
    if (event === "message") {
      self.onmessage = (e) => listener(e.data);
    }
  },
  off: (event: string, listener: (...args: any[]) => void) => {
    if (event === "message") {
      self.onmessage = null;
    }
  },
};
