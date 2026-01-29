import path from "path";
import { workerData } from "../workerThreadsPolyfill";

export function resolvePath(p: string): string {
  // @ts-ignore
  const cwd = self.process?.cwd?.() || workerData?.cwd || "/";
  return path.resolve(cwd, p);
}

export function getFs() {
  // @ts-ignore
  const fs = workerData.fs;
  if (!fs) {
    throw new Error("FS not initialized");
  }
  return fs;
}

export function translateError(error: any, path: string, syscall: string) {
  const message = error.message || String(error);
  if (message.includes("NotFound")) {
    const e = new Error(
      `ENOENT: no such file or directory, ${syscall} '${path}'`,
    );
    (e as any).errno = -2;
    (e as any).code = "ENOENT";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return e;
  }
  return error;
}
