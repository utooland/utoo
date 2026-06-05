import path from "path";
import {
  ERR_ABORT,
  ERR_INVALID_STATE,
  ERR_NO_MODIFICATION_ALLOWED,
  ERR_NOT_ALLOWED,
  ERR_NOT_FOUND,
  ERR_QUOTA_EXCEEDED,
  ERR_TYPE_MISMATCH,
} from "../../../utoo";
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

const preserveOriginalError = (fsError: Error, error: any, message: string) => {
  (fsError as any).originalMessage = message;
  (fsError as any).cause = error;
  return fsError;
};

export function translateError(error: any, path: string, syscall: string) {
  const message = error.message || String(error);

  // 1. NotFound (ENOENT)
  // Mapping "NotFoundError" from tokio-fs-ext
  if (message.includes(ERR_NOT_FOUND())) {
    const e = new Error(
      `ENOENT: no such file or directory, ${syscall} '${path}'`,
    );
    (e as any).errno = -2;
    (e as any).code = "ENOENT";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  // 2. Directory error (Mapped to EISDIR)
  // Mapping "TypeMismatchError" or "type mismatch" from tokio-fs-ext
  if (message.includes(ERR_TYPE_MISMATCH())) {
    const e = new Error(
      `EISDIR: illegal operation on a directory, ${syscall} '${path}'`,
    );
    (e as any).errno = -21;
    (e as any).code = "EISDIR";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  // 3. Locking/Concurrency (Mapped to EAGAIN/EBUSY)
  // Mapping "NoModificationAllowedError" from tokio-fs-ext
  if (message.includes(ERR_NO_MODIFICATION_ALLOWED())) {
    const e = new Error(
      `EAGAIN: resource temporarily unavailable, ${syscall} '${path}'`,
    );
    (e as any).errno = -11;
    (e as any).code = "EAGAIN";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  // 4. Permission Denied (EACCES)
  // Mapping "NotAllowedError" from tokio-fs-ext
  if (message.includes(ERR_NOT_ALLOWED())) {
    const e = new Error(`EACCES: permission denied, ${syscall} '${path}'`);
    (e as any).errno = -13;
    (e as any).code = "EACCES";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  // 5. Storage Full (ENOSPC)
  // Mapping "QuotaExceededError" from tokio-fs-ext
  if (message.includes(ERR_QUOTA_EXCEEDED())) {
    const e = new Error(
      `ENOSPC: no space left on device, ${syscall} '${path}'`,
    );
    (e as any).errno = -28;
    (e as any).code = "ENOSPC";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  // 6. Invalid Argument (EINVAL)
  // Mapping "InvalidStateError" from tokio-fs-ext
  if (message.includes(ERR_INVALID_STATE())) {
    const e = new Error(`EINVAL: invalid argument, ${syscall} '${path}'`);
    (e as any).errno = -22;
    (e as any).code = "EINVAL";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  // 7. Interrupted (EINTR)
  // Mapping "AbortError" from tokio-fs-ext
  if (message.includes(ERR_ABORT())) {
    const e = new Error(`EINTR: interrupted system call, ${syscall} '${path}'`);
    (e as any).errno = -4;
    (e as any).code = "EINTR";
    (e as any).syscall = syscall;
    (e as any).path = path;
    return preserveOriginalError(e, error, message);
  }

  return error;
}
