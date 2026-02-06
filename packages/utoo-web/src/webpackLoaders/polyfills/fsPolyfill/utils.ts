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
  const message = (error.message || String(error)).toLowerCase();

  // 1. NotFound (ENOENT)
  // Mapping "NotFoundError" from tokio-fs-ext
  if (message.includes("notfounderror") || message.includes("notfound")) {
    const e = new Error(
      `ENOENT: no such file or directory, ${syscall} '${path}'`,
    );
    (e as any).errno = -2;
    (e as any).code = "ENOENT";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return e;
  }

  // 2. Directory error (Mapped to EISDIR)
  // Mapping "TypeMismatchError" or "type mismatch" from tokio-fs-ext
  if (message.includes("typemismatcherror") || message.includes("type mismatch")) {
    const e = new Error(
      `EISDIR: illegal operation on a directory, ${syscall} '${path}'`,
    );
    (e as any).errno = -21;
    (e as any).code = "EISDIR";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return e;
  }

  // 3. Locking/Concurrency (Mapped to EAGAIN/EBUSY)
  // Mapping "NoModificationAllowedError" from tokio-fs-ext
  if (
    message.includes("nomodificationallowederror") ||
    message.includes("wouldblock")
  ) {
    const e = new Error(
      `EAGAIN: resource temporarily unavailable, ${syscall} '${path}'`,
    );
    (e as any).errno = -11;
    (e as any).code = "EAGAIN";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return e;
  }

  // 4. Permission Denied (EACCES)
  // Mapping "NotAllowedError" or "SecurityError" from tokio-fs-ext
  if (message.includes("notallowederror") || message.includes("securityerror")) {
    const e = new Error(`EACCES: permission denied, ${syscall} '${path}'`);
    (e as any).errno = -13;
    (e as any).code = "EACCES";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return e;
  }

  // 5. Storage Full (ENOSPC)
  // Mapping "QuotaExceededError" from tokio-fs-ext
  if (message.includes("quotaexceedederror")) {
    const e = new Error(`ENOSPC: no space left on device, ${syscall} '${path}'`);
    (e as any).errno = -28;
    (e as any).code = "ENOSPC";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return e;
  }

  return error;
}
