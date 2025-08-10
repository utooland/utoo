import type { DirEntry } from "./utoo";

export interface ProjectEndpoint {
  install: (packageLock: string) => Promise<void>;
  build: () => Promise<void>;
  readFile(path: string): Promise<string>;
  writeFile(path: string, content: string): Promise<void>;
  readDir(path: string): Promise<DirEntry[]>;
  createDir(path: string): Promise<void>;
  createDirAll(path: string): Promise<void>;
  copyFile(src: string, dst: string): Promise<void>;
}
