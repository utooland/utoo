import { Buffer } from "buffer";
import path from "path";
import * as sabcom from "../../sabcom";
import { RawStats, Stats } from "../../type";

function resolvePath(p: string): string {
  // @ts-ignore
  const cwd = self.process?.cwd?.() || self.workerData?.cwd || "/";
  return path.resolve(cwd, p);
}

function getSabClient() {
  // @ts-ignore
  const client = self.workerData.sabClient;
  if (!client) {
    throw new Error("Sync fs not supported (no sabClient)");
  }
  return client;
}

function getFs() {
  // @ts-ignore
  const fs = self.workerData.fs;
  if (!fs) {
    throw new Error("FS not initialized");
  }
  return fs;
}

// --- Synchronous API (via sabcom) ---

export function readFileSync(path: string, options: any) {
  const client = getSabClient();
  const result = client.call(sabcom.SAB_OP_READ_FILE, resolvePath(path));

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
  const result = client.call(sabcom.SAB_OP_READ_DIR, resolvePath(path));
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
  client.call(sabcom.SAB_OP_WRITE_FILE, payload);
}

export function mkdirSync(path: string, options?: any) {
  const client = getSabClient();
  const recursive = options?.recursive || false;
  const payload = JSON.stringify({ path: resolvePath(path), recursive });
  client.call(sabcom.SAB_OP_MKDIR, payload);
}

export function rmSync(path: string, options?: any) {
  const client = getSabClient();
  const recursive = options?.recursive || false;
  const payload = JSON.stringify({ path: resolvePath(path), recursive });
  client.call(sabcom.SAB_OP_RM, payload);
}

export function rmdirSync(path: string, options?: any) {
  const client = getSabClient();
  const recursive = options?.recursive || false;
  const payload = JSON.stringify({ path: resolvePath(path), recursive });
  client.call(sabcom.SAB_OP_RMDIR, payload);
}

export function copyFileSync(src: string, dst: string) {
  const client = getSabClient();
  const payload = JSON.stringify({
    src: resolvePath(src),
    dst: resolvePath(dst),
  });
  client.call(sabcom.SAB_OP_COPY_FILE, payload);
}

export function statSync(p: string) {
  const client = getSabClient();
  const result = client.call(sabcom.SAB_OP_STAT, resolvePath(p));
  const metadata = JSON.parse(new TextDecoder().decode(result));
  return new Stats({
    type: metadata.type,
    size: Number(metadata.file_size || 0),
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

export const promises = {
  readFile: async (p: string, options?: any) => {
    const encoding =
      options === "utf8" ||
      options === "utf-8" ||
      options?.encoding === "utf8" ||
      options?.encoding === "utf-8"
        ? "utf8"
        : undefined;
    const fs = getFs();
    const path = resolvePath(p);
    const data = await (encoding ? fs.readToString(path) : fs.read(path));
    return encoding ? data : Buffer.from(data);
  },
  writeFile: async (p: string, data: string | Uint8Array, options?: any) => {
    const fs = getFs();
    const path = resolvePath(p);
    if (typeof data === "string") {
      await fs.writeString(path, data);
    } else {
      await fs.write(path, data);
    }
  },
  readdir: async (p: string, options?: any) => {
    const entries = await getFs().readDir(resolvePath(p));
    return entries.map((e: any) => {
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
  },
  mkdir: async (p: string, options?: any) => {
    const fs = getFs();
    const path = resolvePath(p);
    if (options?.recursive) {
      await fs.createDirAll(path);
    } else {
      await fs.createDir(path);
    }
  },
  rm: async (p: string, options?: any) => {
    const fs = getFs();
    const path = resolvePath(p);
    const metadata = await fs.metadata(path);
    const type = (metadata.toJSON() as any).type;
    if (type === "file") {
      await fs.removeFile(path);
    } else {
      await fs.removeDir(path, !!options?.recursive);
    }
  },
  rmdir: async (p: string, options?: any) =>
    getFs().removeDir(resolvePath(p), !!options?.recursive),
  copyFile: async (src: string, dst: string) =>
    getFs().copyFile(resolvePath(src), resolvePath(dst)),
  stat: async (p: string) => {
    const metadata = await getFs().metadata(resolvePath(p));
    const json = metadata.toJSON() as any;
    return new Stats({
      type: json.type,
      size: Number(json.file_size || 0),
    });
  },
  lstat: async (p: string) => {
    const metadata = await getFs().metadata(resolvePath(p));
    const json = metadata.toJSON() as any;
    return new Stats({
      type: json.type,
      size: Number(json.file_size || 0),
    });
  },
  realpath: async (p: string) => p,
  access: async (p: string, mode?: number) => {
    await getFs().metadata(resolvePath(p));
  },
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
