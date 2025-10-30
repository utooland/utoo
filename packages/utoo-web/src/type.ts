import { Issue } from "@utoo/pack-shared";
import { DirEntryType } from "./utoo";

export interface RawDirent {
  name: string;
  type: DirEntryType;
}

export class Dirent {
  public name: string;

  constructor(private rawDirent: RawDirent) {
    this.name = this.rawDirent.name;
  }

  public isDirectory() {
    return this.rawDirent.type === "directory";
  }

  public isFile() {
    return this.rawDirent.type === "file";
  }
}

export interface BuildOutput {
  issues: Issue[];
}

export interface ProjectEndpoint {
  install: (packageLock: string) => Promise<void>;
  build: () => Promise<BuildOutput>;
  readFile(path: string): Promise<Uint8Array>;
  readFile(path: string, encoding?: "utf8"): Promise<string>;
  writeFile(
    path: string,
    content: string | Uint8Array,
    encoding?: "utf8",
  ): Promise<void>;
  readdir(path: string, options?: { recursive?: boolean }): Promise<Dirent[]>;
  mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
  rm(path: string, options?: { recursive?: boolean }): Promise<void>;
  rmdir(path: string, options?: { recursive?: boolean }): Promise<void>;
  copyFile(src: string, dst: string): Promise<void>;
}

export interface ProjectOptions {
  cwd: string;
  workerUrl?: string;
  threadWorkerUrl: string;
  wasmUrl?: string;
  serviceWorker?: ServiceWorkerOptions;
  logFilter?: string;
}

export interface ServiceWorkerOptions {
  url: string;
  scope: string;
}
