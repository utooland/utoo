import { Buffer } from "buffer";
import { Stats } from "../../../types";
import { getFs, resolvePath, translateError } from "./utils";

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
    const resolvedPath = resolvePath(p);
    try {
      const data = await (encoding
        ? fs.readToString(resolvedPath)
        : fs.read(resolvedPath));
      return encoding ? data : Buffer.from(data);
    } catch (e) {
      throw translateError(e, p, "open");
    }
  },
  writeFile: async (p: string, data: string | Uint8Array, options?: any) => {
    const fs = getFs();
    const resolvedPath = resolvePath(p);
    try {
      if (typeof data === "string") {
        await fs.writeString(resolvedPath, data);
      } else {
        await fs.write(resolvedPath, data);
      }
    } catch (e) {
      throw translateError(e, p, "open");
    }
  },
  readdir: async (p: string, options?: any) => {
    const resolvedPath = resolvePath(p);
    try {
      const entries = await getFs().readDir(resolvedPath);
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
    } catch (e) {
      throw translateError(e, p, "scandir");
    }
  },
  mkdir: async (p: string, options?: any) => {
    const fs = getFs();
    const resolvedPath = resolvePath(p);
    try {
      if (options?.recursive) {
        await fs.createDirAll(resolvedPath);
      } else {
        await fs.createDir(resolvedPath);
      }
    } catch (e) {
      throw translateError(e, p, "mkdir");
    }
  },
  rm: async (p: string, options?: any) => {
    const fs = getFs();
    const resolvedPath = resolvePath(p);
    try {
      const metadata = await fs.metadata(resolvedPath);
      const type = (metadata.toJSON() as any).type;
      if (type === "file") {
        await fs.removeFile(resolvedPath);
      } else {
        await fs.removeDir(resolvedPath, !!options?.recursive);
      }
    } catch (e) {
      throw translateError(e, p, "rm");
    }
  },
  rmdir: async (p: string, options?: any) => {
    const resolvedPath = resolvePath(p);
    try {
      return await getFs().removeDir(resolvedPath, !!options?.recursive);
    } catch (e) {
      throw translateError(e, p, "rmdir");
    }
  },
  copyFile: async (src: string, dst: string) => {
    try {
      return await getFs().copyFile(resolvePath(src), resolvePath(dst));
    } catch (e) {
      throw translateError(e, src, "copyfile");
    }
  },
  stat: async (p: string) => {
    const resolvedPath = resolvePath(p);
    try {
      const metadata = await getFs().metadata(resolvedPath);
      const json = metadata.toJSON() as any;
      return new Stats({
        type: json.type,
        size: Number(json.file_size || 0),
      });
    } catch (e) {
      throw translateError(e, p, "stat");
    }
  },
  lstat: async (p: string) => {
    const resolvedPath = resolvePath(p);
    try {
      const metadata = await getFs().metadata(resolvedPath);
      const json = metadata.toJSON() as any;
      return new Stats({
        type: json.type,
        size: Number(json.file_size || 0),
      });
    } catch (e) {
      throw translateError(e, p, "lstat");
    }
  },
  realpath: async (p: string) => p,
  access: async (p: string, mode?: number) => {
    const resolvedPath = resolvePath(p);
    try {
      await getFs().metadata(resolvedPath);
    } catch (e) {
      throw translateError(e, p, "access");
    }
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
