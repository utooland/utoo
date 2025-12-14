// @ts-ignore
export const workerData = self.workerData;
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
