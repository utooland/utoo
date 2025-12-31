import { Buffer } from "buffer";
import path from "path";
import { Stats } from "../../types";
import { promises } from "./fsPromisesPolyfill";
import { workerData } from "./workerThreadsPolyfill";

function resolvePath(p: string): string {
  // @ts-ignore
  const cwd = self.process?.cwd?.() || workerData?.cwd || "/";
  return path.resolve(cwd, p);
}

function getFs() {
  // @ts-ignore
  const fs = workerData.fs;
  if (!fs) {
    throw new Error("FS not initialized");
  }
  return fs;
}

// --- Synchronous API (via WASM Project Sync APIs) ---

const textDecoder = new TextDecoder();

export function readFileSync(path: string, options: any) {
  const fs = getFs();
  const result = fs.readSync(resolvePath(path));

  if (
    options === "utf8" ||
    options === "utf-8" ||
    (options && (options.encoding === "utf8" || options.encoding === "utf-8"))
  ) {
    return textDecoder.decode(result);
  }
  return Buffer.from(result);
}

export function readdirSync(path: string, options?: any) {
  const fs = getFs();
  const entries = fs.readDirSync(resolvePath(path));

  if (options?.withFileTypes) {
    return entries.map((e: any) => ({
      name: e.name,
      isFile: () => e.type === "file",
      isDirectory: () => e.type === "directory",
      isSymbolicLink: () => false,
    }));
  }
  return entries.map((e: any) => e.name);
}

export function writeFileSync(
  path: string,
  data: string | Uint8Array,
  options?: any,
) {
  const fs = getFs();
  let content: Uint8Array;
  if (typeof data === "string") {
    content = new TextEncoder().encode(data);
  } else {
    content = data;
  }
  fs.writeSync(resolvePath(path), content);
}

export function mkdirSync(path: string, options?: any) {
  const fs = getFs();
  const recursive = options?.recursive || false;
  if (recursive) {
    fs.createDirAllSync(resolvePath(path));
  } else {
    fs.createDirSync(resolvePath(path));
  }
}

export function rmSync(path: string, options?: any) {
  const fs = getFs();
  const recursive = options?.recursive || false;
  if (recursive) {
    fs.removeDirSync(resolvePath(path), true);
  } else {
    fs.removeFileSync(resolvePath(path));
  }
}

export function rmdirSync(path: string, options?: any) {
  const fs = getFs();
  const recursive = options?.recursive || false;
  fs.removeDirSync(resolvePath(path), recursive);
}

export function copyFileSync(src: string, dst: string) {
  const fs = getFs();
  fs.copyFileSync(resolvePath(src), resolvePath(dst));
}

export function statSync(p: string) {
  const fs = getFs();
  const metadata = fs.metadataSync(resolvePath(p));

  let type, size;
  // @ts-ignore
  if (typeof metadata.toJSON === "function") {
    // @ts-ignore
    const json = metadata.toJSON();
    type = json.type;
    size = json.file_size;
  } else {
    // @ts-ignore
    type = metadata.type;
    // @ts-ignore
    size = metadata.file_size;
  }

  return new Stats({
    type: type === "directory" ? "directory" : "file",
    size: Number(size || 0),
    atimeMs: Date.now(),
    mtimeMs: Date.now(),
    ctimeMs: Date.now(),
    birthtimeMs: Date.now(),
  });
}

export function lstatSync(p: string) {
  return statSync(p);
}

export function realpathSync(p: string) {
  return p;
}

export function accessSync(path: string, mode?: number) {
  statSync(path);
}

export function existsSync(path: string) {
  try {
    statSync(path);
    return true;
  } catch (e) {
    return false;
  }
}

// --- Asynchronous API (via WASM Project) ---

export function readFile(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  const encoding =
    options === "utf8" ||
    options === "utf-8" ||
    options?.encoding === "utf8" ||
    options?.encoding === "utf-8"
      ? "utf8"
      : undefined;

  const p = resolvePath(path);
  const fs = getFs();

  const promise = encoding ? fs.readToString(p) : fs.read(p);

  promise
    .then((data: any) => {
      cb(null, encoding ? data : Buffer.from(data));
    })
    .catch((e: any) => cb(e));
}

export function readdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  getFs()
    .readDir(resolvePath(path))
    .then((entries: any[]) => {
      const result = entries.map((e: any) => {
        const json = e.toJSON() as any;
        if (options?.withFileTypes) {
          return {
            name: json.name,
            isFile: () => json.type === "file",
            isDirectory: () => json.type === "directory",
            isSymbolicLink: () => false,
          };
        }
        return json.name;
      });
      cb(null, result);
    })
    .catch((e: any) => cb(e));
}

export function writeFile(
  path: string,
  data: string | Uint8Array,
  options: any,
  cb: Function,
) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  const p = resolvePath(path);
  const fs = getFs();

  const promise =
    typeof data === "string" ? fs.writeString(p, data) : fs.write(p, data);

  promise.then(() => cb(null)).catch((e: any) => cb(e));
}

export function mkdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  const p = resolvePath(path);
  const fs = getFs();
  const promise = options?.recursive ? fs.createDirAll(p) : fs.createDir(p);

  promise.then(() => cb(null)).catch((e: any) => cb(e));
}

export function rm(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  const p = resolvePath(path);
  const fs = getFs();

  fs.metadata(p)
    .then((metadata: any) => {
      const type = (metadata.toJSON() as any).type;
      if (type === "file") {
        return fs.removeFile(p);
      } else {
        return fs.removeDir(p, !!options?.recursive);
      }
    })
    .then(() => cb(null))
    .catch((e: any) => cb(e));
}

export function rmdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  getFs()
    .removeDir(resolvePath(path), !!options?.recursive)
    .then(() => cb(null))
    .catch((e: any) => cb(e));
}

export function copyFile(src: string, dst: string, cb: Function) {
  getFs()
    .copyFile(resolvePath(src), resolvePath(dst))
    .then(() => cb(null))
    .catch((e: any) => cb(e));
}

export function stat(p: string, cb: Function) {
  getFs()
    .metadata(resolvePath(p))
    .then((metadata: any) => {
      const json = metadata.toJSON() as any;
      cb(
        null,
        new Stats({
          type: json.type,
          size: Number(json.file_size || 0),
        }),
      );
    })
    .catch((e: any) => cb(e));
}

export function lstat(p: string, cb: Function) {
  stat(p, cb);
}

export function realpath(p: string, cb: Function) {
  cb(null, p);
}

export function access(p: string, mode: number | Function, cb?: Function) {
  if (typeof mode === "function") {
    cb = mode;
    mode = 0;
  }
  stat(p, (err: any) => {
    if (cb) cb(err);
  });
}

export const constants = {
  F_OK: 0,
  R_OK: 4,
  W_OK: 2,
  X_OK: 1,
};

export default {
  readFile,
  readFileSync,
  readdir,
  readdirSync,
  writeFile,
  writeFileSync,
  mkdir,
  mkdirSync,
  rm,
  rmSync,
  rmdir,
  rmdirSync,
  copyFile,
  copyFileSync,
  stat,
  statSync,
  lstat,
  lstatSync,
  realpath,
  realpathSync,
  access,
  accessSync,
  existsSync,
  promises,
  constants,
};

export { promises };
