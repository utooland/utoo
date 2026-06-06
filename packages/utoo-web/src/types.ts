import { ConfigComplete, Issue, type UpdateMessage } from "@utoo/pack-shared";
import initWasm, { DirEntryType } from "./utoo";

export { type UpdateMessage } from "@utoo/pack-shared";

export interface RawDirent {
  name: string;
  type: DirEntryType;
}

export interface RawStats {
  type: DirEntryType;
  size: number;
  atimeMs?: number;
  mtimeMs?: number;
  ctimeMs?: number;
  birthtimeMs?: number;
}

export class Stats {
  public dev = 0;
  public ino = 0;
  public mode: number;
  public nlink = 1;
  public uid = 0;
  public gid = 0;
  public rdev = 0;
  public size: number;
  public blksize = 4096;
  public blocks = 0;
  public atimeMs: number;
  public mtimeMs: number;
  public ctimeMs: number;
  public birthtimeMs: number;
  public atime: Date;
  public mtime: Date;
  public ctime: Date;
  public birthtime: Date;

  constructor(private raw: RawStats) {
    this.size = raw.size;
    this.mode = raw.type === "directory" ? 16877 : 33188;
    this.atimeMs = raw.atimeMs || 0;
    this.mtimeMs = raw.mtimeMs || 0;
    this.ctimeMs = raw.ctimeMs || 0;
    this.birthtimeMs = raw.birthtimeMs || 0;
    this.atime = new Date(this.atimeMs);
    this.mtime = new Date(this.mtimeMs);
    this.ctime = new Date(this.ctimeMs);
    this.birthtime = new Date(this.birthtimeMs);
  }

  public isDirectory() {
    return this.raw.type === "directory";
  }

  public isFile() {
    return this.raw.type === "file";
  }

  public isSymbolicLink() {
    return false;
  }

  public isBlockDevice() {
    return false;
  }

  public isCharacterDevice() {
    return false;
  }

  public isFIFO() {
    return false;
  }

  public isSocket() {
    return false;
  }
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

export interface PackFile {
  path: string;
  content: Uint8Array;
}

export interface DepsOptions {
  registry?: string | null;
  concurrency?: number | null;
}

export type OmitType = "dev" | "optional";

export interface InstallOptions {
  maxConcurrentDownloads?: number;
  /** Dependency types to omit. Default: [] */
  omit?: OmitType[];
}

export interface ProjectEndpoint {
  deps: (options?: DepsOptions) => Promise<string>;
  install: (packageLock: string, options?: InstallOptions) => Promise<void>;
  build: (options?: {
    config?: ConfigComplete;
    cleanup?: boolean;
  }) => Promise<BuildOutput>;
  /** Start dev mode with file watching. onUpdate is called on each rebuild. */
  dev: (options?: {
    config?: ConfigComplete;
    onUpdate?: (result: BuildOutput) => void;
  }) => void;
  /**
   * Subscribe to HMR events for a specific identifier.
   * Returns a Promise that resolves when the subscription is set up.
   * Matches NAPI signature: project_hmr_events(project, identifier, func) -> RootTask
   * Callback receives: { result: ClientUpdateInstruction, issues: Issue[], diagnostics: Diagnostic[] }
   */
  hmrSubscribe: (
    identifier: string,
    callback: (update: unknown) => void,
  ) => void | Promise<void>;
  /** Subscribe to compilation lifecycle events (start/end). */
  updateInfoSubscribe: (
    aggregationMs: number,
    callback: (message: UpdateMessage) => void,
  ) => void;
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
  stat(path: string): Promise<Stats>;
  gzip: (files: PackFile[]) => Promise<Uint8Array>;
  sigMd5: (content: Uint8Array) => Promise<string>;
}

export interface ProjectOptions {
  cwd: string;
  workerUrl: string;
  threadWorkerUrl: string;
  loaderWorkerUrl?: string;
  wasmUrl?: string;
  serviceWorker?: ServiceWorkerOptions;
  logFilter?: string;
  loadersImportMap?: Record<string, string>;
  loadersImportMapFetchCache?: RequestCache;
}

export interface ServiceWorkerOptions {
  url: string;
  scope: string;
  targetDirToCwd?: string;
}

export type Binding = Awaited<ReturnType<typeof initWasm>>;
