import { Buffer } from "buffer";
import { Stats } from "../../../types";
import { promises } from "./promises";
import { getFs, resolvePath, translateError } from "./utils";

// --- Synchronous API (via WASM Project Sync APIs) ---

const textDecoder = new TextDecoder();

export function readFileSync(path: string, options: any) {
  const fs = getFs();
  const resolvedPath = resolvePath(path);
  try {
    const result = fs.readSync(resolvedPath);

    if (
      options === "utf8" ||
      options === "utf-8" ||
      (options && (options.encoding === "utf8" || options.encoding === "utf-8"))
    ) {
      return textDecoder.decode(result);
    }
    return Buffer.from(result);
  } catch (e) {
    throw translateError(e, path, "open");
  }
}

export function readdirSync(path: string, options?: any) {
  const fs = getFs();
  const resolvedPath = resolvePath(path);
  try {
    const entries = fs.readDirSync(resolvedPath);

    if (options?.withFileTypes) {
      return entries.map((e: any) => ({
        name: e.name,
        isFile: () => e.type === "file",
        isDirectory: () => e.type === "directory",
        isSymbolicLink: () => false,
      }));
    }
    return entries.map((e: any) => e.name);
  } catch (e) {
    throw translateError(e, path, "scandir");
  }
}

export function writeFileSync(
  path: string,
  data: string | Uint8Array,
  options?: any,
) {
  const fs = getFs();
  const resolvedPath = resolvePath(path);
  try {
    let content: Uint8Array;
    if (typeof data === "string") {
      content = new TextEncoder().encode(data);
    } else {
      content = data;
    }
    fs.writeSync(resolvedPath, content);
  } catch (e) {
    throw translateError(e, path, "open");
  }
}

export function mkdirSync(path: string, options?: any) {
  const fs = getFs();
  const resolvedPath = resolvePath(path);
  try {
    const recursive = options?.recursive || false;
    if (recursive) {
      fs.createDirAllSync(resolvedPath);
    } else {
      fs.createDirSync(resolvedPath);
    }
  } catch (e) {
    throw translateError(e, path, "mkdir");
  }
}

export function rmSync(path: string, options?: any) {
  const fs = getFs();
  const resolvedPath = resolvePath(path);
  try {
    const recursive = options?.recursive || false;
    if (recursive) {
      fs.removeDirSync(resolvedPath, true);
    } else {
      fs.removeFileSync(resolvedPath);
    }
  } catch (e) {
    throw translateError(e, path, "rm");
  }
}

export function rmdirSync(path: string, options?: any) {
  const fs = getFs();
  const resolvedPath = resolvePath(path);
  try {
    const recursive = options?.recursive || false;
    fs.removeDirSync(resolvedPath, recursive);
  } catch (e) {
    throw translateError(e, path, "rmdir");
  }
}

export function copyFileSync(src: string, dst: string) {
  const fs = getFs();
  try {
    fs.copyFileSync(resolvePath(src), resolvePath(dst));
  } catch (e) {
    throw translateError(e, src, "copyfile");
  }
}

export function statSync(p: string) {
  const fs = getFs();
  const resolvedPath = resolvePath(p);
  try {
    const metadata = fs.metadataSync(resolvedPath);

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
  } catch (e) {
    throw translateError(e, p, "stat");
  }
}

export function lstatSync(p: string) {
  const fs = getFs();
  const resolvedPath = resolvePath(p);
  try {
    const metadata = fs.metadataSync(resolvedPath);

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
  } catch (e) {
    throw translateError(e, p, "lstat");
  }
}

export function realpathSync(p: string) {
  return p;
}

export function accessSync(p: string, mode?: number) {
  const fs = getFs();
  const resolvedPath = resolvePath(p);
  try {
    fs.metadataSync(resolvedPath);
  } catch (e) {
    throw translateError(e, p, "access");
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

  const resolvedPath = resolvePath(path);
  const fs = getFs();

  const promise = encoding
    ? fs.readToString(resolvedPath)
    : fs.read(resolvedPath);

  promise
    .then((data: any) => {
      cb(null, encoding ? data : Buffer.from(data));
    })
    .catch((e: any) => cb(translateError(e, path, "open")));
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
    .catch((e: any) => cb(translateError(e, path, "scandir")));
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
  const resolvedPath = resolvePath(path);
  const fs = getFs();

  const promise =
    typeof data === "string"
      ? fs.writeString(resolvedPath, data)
      : fs.write(resolvedPath, data);

  promise
    .then(() => cb(null))
    .catch((e: any) => cb(translateError(e, path, "open")));
}

export function mkdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  const resolvedPath = resolvePath(path);
  const fs = getFs();
  const promise = options?.recursive
    ? fs.createDirAll(resolvedPath)
    : fs.createDir(resolvedPath);

  promise
    .then(() => cb(null))
    .catch((e: any) => cb(translateError(e, path, "mkdir")));
}

export function rm(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  const resolvedPath = resolvePath(path);
  const fs = getFs();

  fs.metadata(resolvedPath)
    .then((metadata: any) => {
      const type = (metadata.toJSON() as any).type;
      if (type === "file") {
        return fs.removeFile(resolvedPath);
      } else {
        return fs.removeDir(resolvedPath, !!options?.recursive);
      }
    })
    .then(() => cb(null))
    .catch((e: any) => cb(translateError(e, path, "rm")));
}

export function rmdir(path: string, options: any, cb: Function) {
  if (typeof options === "function") {
    cb = options;
    options = {};
  }
  getFs()
    .removeDir(resolvePath(path), !!options?.recursive)
    .then(() => cb(null))
    .catch((e: any) => cb(translateError(e, path, "rmdir")));
}

export function copyFile(src: string, dst: string, cb: Function) {
  getFs()
    .copyFile(resolvePath(src), resolvePath(dst))
    .then(() => cb(null))
    .catch((e: any) => cb(translateError(e, src, "copyfile")));
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
    .catch((e: any) => cb(translateError(e, p, "stat")));
}

export function lstat(p: string, cb: Function) {
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
    .catch((e: any) => cb(translateError(e, p, "lstat")));
}

export function realpath(p: string, cb: Function) {
  cb(null, p);
}

export function access(p: string, mode: number | Function, cb?: Function) {
  if (typeof mode === "function") {
    cb = mode;
    mode = 0;
  }
  getFs()
    .metadata(resolvePath(p))
    .then(() => {
      if (cb) cb(null);
    })
    .catch((e: any) => {
      if (cb) cb(translateError(e, p, "access"));
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
