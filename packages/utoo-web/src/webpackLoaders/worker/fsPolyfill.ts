
export function readFile(path: string, options: any, cb: Function) {
  if (typeof options === 'function') {
    cb = options;
    options = {};
  }
  // @ts-ignore
  return self.workerData.readFile(path, options).then(
    (data: any) => cb(null, data),
    (err: Error) => cb(err),
  );
}
