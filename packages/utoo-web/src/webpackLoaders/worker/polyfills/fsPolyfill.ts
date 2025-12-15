import { Buffer } from "buffer";
import path from "path";
import {
  SAB_OP_COPY_FILE,
  SAB_OP_MKDIR,
  SAB_OP_READ_DIR,
  SAB_OP_READ_FILE,
  SAB_OP_RM,
  SAB_OP_RMDIR,
  SAB_OP_WRITE_FILE,
} from "../../../sabcom";

function resolvePath(p: string): string {
  // @ts-ignore
  const cwd = self.process?.cwd?.() || self.workerData?.cwd || "/";

  if (cwd === "/") {
    if (p.startsWith("/")) {
      return p.slice(1);
    }
    return p;
  }

  if (p.startsWith(cwd)) {
    // Check if it's an exact match or followed by separator
    if (p.length === cwd.length) {
      return ".";
    }
    if (p[cwd.length] === path.sep || p[cwd.length] === "/") {
      let relative = p.slice(cwd.length);
      if (relative.startsWith("/")) relative = relative.slice(1);
      return relative || ".";
    }
  }
  return p;
}

function getSabClient() {
  // @ts-ignore
  const client = self.workerData.sabClient;
  if (!client) {
    throw new Error("Sync fs not supported (no sabClient)");
  }
  return client;
}

export function readFile(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  try {
    const data = readFileSync(path, options);
    cb(null, data);
  } catch (e) {
    cb(e);
  }
}

export function readFileSync(path: string, options: any) {
  const client = getSabClient();
  const result = client.call(SAB_OP_READ_FILE, resolvePath(path));

  if (
    options === "utf8" ||
    options === "utf-8" ||
    (options && (options.encoding === "utf8" || options.encoding === "utf-8"))
  ) {
    return new TextDecoder().decode(result);
  }
  return Buffer.from(result);
}

export function readdirSync(path: string, options?: any) {
  const client = getSabClient();
  const result = client.call(SAB_OP_READ_DIR, resolvePath(path));
  const json = new TextDecoder().decode(result);
  const entries = JSON.parse(json);

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

export function readdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  try {
    const files = readdirSync(path, options);
    cb(null, files);
  } catch (e) {
    cb(e);
  }
}

export function writeFileSync(
  path: string,
  data: string | Uint8Array,
  options?: any,
) {
  const client = getSabClient();
  // TODO: handle binary data properly
  const content =
    typeof data === "string" ? data : new TextDecoder().decode(data);
  const payload = JSON.stringify({ path: resolvePath(path), data: content });
  client.call(SAB_OP_WRITE_FILE, payload);
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
  try {
    writeFileSync(path, data, options);
    cb(null);
  } catch (e) {
    cb(e);
  }
}

export function mkdirSync(path: string, options?: any) {
  const client = getSabClient();
  const recursive = options?.recursive || false;
  const payload = JSON.stringify({ path: resolvePath(path), recursive });
  client.call(SAB_OP_MKDIR, payload);
}

export function mkdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  try {
    mkdirSync(path, options);
    cb(null);
  } catch (e) {
    cb(e);
  }
}

export function rmSync(path: string, options?: any) {
  const client = getSabClient();
  const recursive = options?.recursive || false;
  const payload = JSON.stringify({ path: resolvePath(path), recursive });
  client.call(SAB_OP_RM, payload);
}

export function rm(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  try {
    rmSync(path, options);
    cb(null);
  } catch (e) {
    cb(e);
  }
}

export function rmdirSync(path: string, options?: any) {
  const client = getSabClient();
  const recursive = options?.recursive || false;
  const payload = JSON.stringify({ path: resolvePath(path), recursive });
  client.call(SAB_OP_RMDIR, payload);
}

export function rmdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  try {
    rmdirSync(path, options);
    cb(null);
  } catch (e) {
    cb(e);
  }
}

export function copyFileSync(src: string, dst: string) {
  const client = getSabClient();
  const payload = JSON.stringify({
    src: resolvePath(src),
    dst: resolvePath(dst),
  });
  client.call(SAB_OP_COPY_FILE, payload);
}

export function copyFile(src: string, dst: string, cb: Function) {
  try {
    copyFileSync(src, dst);
    cb(null);
  } catch (e) {
    cb(e);
  }
}

export function statSync(p: string) {
  const parent = path.dirname(p);
  const name = path.basename(p);

  // Root check
  if (p === "/" || p === ".") {
    const now = new Date();
    const nowMs = now.getTime();
    return {
      dev: 0,
      ino: 0,
      mode: 16877,
      nlink: 1,
      uid: 0,
      gid: 0,
      rdev: 0,
      size: 0,
      blksize: 4096,
      blocks: 0,
      atimeMs: nowMs,
      mtimeMs: nowMs,
      ctimeMs: nowMs,
      birthtimeMs: nowMs,
      atime: now,
      mtime: now,
      ctime: now,
      birthtime: now,
      isDirectory: () => true,
      isFile: () => false,
      isSymbolicLink: () => false,
      isBlockDevice: () => false,
      isCharacterDevice: () => false,
      isFIFO: () => false,
      isSocket: () => false,
    };
  }

  try {
    const entries = readdirSync(parent, { withFileTypes: true });
    const entry = entries.find((e: any) => e.name === name);
    if (!entry) {
      const error = new Error(`ENOENT: no such file or directory, stat '${p}'`);
      // @ts-ignore
      error.code = "ENOENT";
      // @ts-ignore
      error.errno = -2;
      // @ts-ignore
      error.syscall = "stat";
      // @ts-ignore
      error.path = p;
      throw error;
    }
    const now = new Date();
    const nowMs = now.getTime();
    return {
      dev: 0,
      ino: 0,
      mode: entry.isDirectory() ? 16877 : 33188,
      nlink: 1,
      uid: 0,
      gid: 0,
      rdev: 0,
      size: 0,
      blksize: 4096,
      blocks: 0,
      atimeMs: nowMs,
      mtimeMs: nowMs,
      ctimeMs: nowMs,
      birthtimeMs: nowMs,
      atime: now,
      mtime: now,
      ctime: now,
      birthtime: now,
      isDirectory: () => entry.isDirectory(),
      isFile: () => entry.isFile(),
      isSymbolicLink: () => entry.isSymbolicLink(),
      isBlockDevice: () => false,
      isCharacterDevice: () => false,
      isFIFO: () => false,
      isSocket: () => false,
    };
  } catch (e) {
    throw e;
  }
}

export function lstatSync(p: string) {
  return statSync(p);
}

export function stat(path: string, cb: Function) {
  try {
    const stats = statSync(path);
    cb(null, stats);
  } catch (e) {
    cb(e);
  }
}

export function lstat(path: string, cb: Function) {
  try {
    const stats = lstatSync(path);
    cb(null, stats);
  } catch (e) {
    cb(e);
  }
}

export function realpathSync(p: string) {
  return p;
}

export function realpath(p: string, cb: Function) {
  cb(null, p);
}

export function accessSync(path: string, mode?: number) {
  try {
    statSync(path);
  } catch (e) {
    throw e;
  }
}

export function access(path: string, mode: number | Function, cb?: Function) {
  if (typeof mode === "function") {
    cb = mode;
    mode = 0;
  }
  try {
    accessSync(path, mode as number);
    if (cb) cb(null);
  } catch (e) {
    if (cb) cb(e);
  }
}

export function existsSync(path: string) {
  try {
    statSync(path);
    return true;
  } catch (e) {
    return false;
  }
}

export const promises = {
  readFile: async (path: string, options?: any) => readFileSync(path, options),
  writeFile: async (path: string, data: string | Uint8Array, options?: any) =>
    writeFileSync(path, data, options),
  readdir: async (path: string, options?: any) => readdirSync(path, options),
  mkdir: async (path: string, options?: any) => mkdirSync(path, options),
  rm: async (path: string, options?: any) => rmSync(path, options),
  rmdir: async (path: string, options?: any) => rmdirSync(path, options),
  copyFile: async (src: string, dst: string) => copyFileSync(src, dst),
  stat: async (p: string) => statSync(p),
  lstat: async (p: string) => lstatSync(p),
  realpath: async (p: string) => realpathSync(p),
  access: async (path: string, mode?: number) => accessSync(path, mode),
};

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
