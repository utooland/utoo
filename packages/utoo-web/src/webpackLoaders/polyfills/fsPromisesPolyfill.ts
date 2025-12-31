import { Buffer } from "buffer";
import path from "path";
import { Stats } from "../../types";
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

export const readFile = promises.readFile;
export const writeFile = promises.writeFile;
export const readdir = promises.readdir;
export const mkdir = promises.mkdir;
export const rm = promises.rm;
export const rmdir = promises.rmdir;
export const copyFile = promises.copyFile;
export const stat = promises.stat;
export const lstat = promises.lstat;
export const realpath = promises.realpath;
export const access = promises.access;

export default promises;
