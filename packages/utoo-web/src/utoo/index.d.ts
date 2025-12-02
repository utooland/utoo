/* tslint:disable */
/* eslint-disable */
export function init_pack(): void;
export function init_log_filter(filter: string): void;
export function recvWorkerRequest(pool_id: string): Promise<number>;
export function sendTaskMessage(task_id: number, message: string): Promise<void>;
export function recvPoolRequest(): Promise<PoolOptions>;
export function recvMessageInWorker(worker_id: number): Promise<string>;
export function notifyWorkerAck(task_id: number, worker_id: number): Promise<void>;
export function recvWorkerTermination(): Promise<WorkerTermination>;
/**
 * Entry point for web workers
 */
export function wasm_thread_entry_point(ptr: number): void;
type DirEntryType = "file" | "directory";
export class CreateSyncAccessHandleOptions {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
}
export class DirEntry {
  private constructor();
/**
** Return copy of self without private attributes.
*/
  toJSON(): Object;
/**
* Return stringified version of self.
*/
  toString(): string;
  free(): void;
  [Symbol.dispose](): void;
  name: string;
  type: DirEntryType;
}
export class Metadata {
  private constructor();
/**
** Return copy of self without private attributes.
*/
  toJSON(): Object;
/**
* Return stringified version of self.
*/
  toString(): string;
  free(): void;
  [Symbol.dispose](): void;
}
export class PoolOptions {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  filename: string;
  maxConcurrency: number;
}
export class Project {
  free(): void;
  [Symbol.dispose](): void;
  createDir(path: string): Promise<void>;
  removeDir(path: string, recursive: boolean): Promise<void>;
  removeFile(path: string): Promise<void>;
  writeString(path: string, content: string): Promise<void>;
  createDirAll(path: string): Promise<void>;
  readToString(path: string): Promise<string>;
  constructor(cwd: string, thread_url: string);
  /**
   * Create a tar.gz archive and return bytes (no file I/O)
   * This is useful for main thread execution without OPFS access
   */
  gzip(files: any): Uint8Array;
  read(path: string): Promise<Uint8Array>;
  build(): Promise<any>;
  write(path: string, content: Uint8Array): Promise<void>;
  install(package_lock: string, max_concurrent_downloads?: number | null): Promise<void>;
  /**
   * Calculate MD5 hash of byte content
   */
  sigMd5(content: Uint8Array): string;
  metadata(path: string): Promise<Metadata>;
  readDir(path: string): Promise<DirEntry[]>;
  copyFile(src: string, dst: string): Promise<void>;
  readonly cwd: string;
}
export class WorkerTermination {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  filename: string;
  worker_id: number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly __wbg_direntry_free: (a: number, b: number) => void;
  readonly __wbg_get_direntry_name: (a: number) => [number, number];
  readonly __wbg_get_direntry_type: (a: number) => number;
  readonly __wbg_metadata_free: (a: number, b: number) => void;
  readonly __wbg_project_free: (a: number, b: number) => void;
  readonly __wbg_set_direntry_name: (a: number, b: number, c: number) => void;
  readonly __wbg_set_direntry_type: (a: number, b: number) => void;
  readonly project_build: (a: number) => any;
  readonly project_copyFile: (a: number, b: number, c: number, d: number, e: number) => any;
  readonly project_createDir: (a: number, b: number, c: number) => any;
  readonly project_createDirAll: (a: number, b: number, c: number) => any;
  readonly project_cwd: (a: number) => [number, number];
  readonly project_gzip: (a: number, b: any) => [number, number, number];
  readonly project_install: (a: number, b: number, c: number, d: number) => any;
  readonly project_metadata: (a: number, b: number, c: number) => any;
  readonly project_new: (a: number, b: number, c: number, d: number) => number;
  readonly project_read: (a: number, b: number, c: number) => any;
  readonly project_readDir: (a: number, b: number, c: number) => any;
  readonly project_readToString: (a: number, b: number, c: number) => any;
  readonly project_removeDir: (a: number, b: number, c: number, d: number) => any;
  readonly project_removeFile: (a: number, b: number, c: number) => any;
  readonly project_sigMd5: (a: number, b: number, c: number) => [number, number];
  readonly project_write: (a: number, b: number, c: number, d: number, e: number) => any;
  readonly project_writeString: (a: number, b: number, c: number, d: number, e: number) => any;
  readonly init_log_filter: (a: number, b: number) => void;
  readonly init_pack: () => void;
  readonly __wbg_get_pooloptions_filename: (a: number) => [number, number];
  readonly __wbg_get_pooloptions_maxConcurrency: (a: number) => number;
  readonly __wbg_get_workertermination_filename: (a: number) => [number, number];
  readonly __wbg_get_workertermination_worker_id: (a: number) => number;
  readonly __wbg_pooloptions_free: (a: number, b: number) => void;
  readonly __wbg_set_pooloptions_filename: (a: number, b: number, c: number) => void;
  readonly __wbg_set_pooloptions_maxConcurrency: (a: number, b: number) => void;
  readonly __wbg_set_workertermination_filename: (a: number, b: number, c: number) => void;
  readonly __wbg_set_workertermination_worker_id: (a: number, b: number) => void;
  readonly __wbg_workertermination_free: (a: number, b: number) => void;
  readonly notifyWorkerAck: (a: number, b: number) => any;
  readonly recvMessageInWorker: (a: number) => any;
  readonly recvPoolRequest: () => any;
  readonly recvWorkerRequest: (a: number, b: number) => any;
  readonly recvWorkerTermination: () => any;
  readonly sendTaskMessage: (a: number, b: number, c: number) => any;
  readonly __wbg_createsyncaccesshandleoptions_free: (a: number, b: number) => void;
  readonly wasm_thread_entry_point: (a: number) => void;
  readonly memory: WebAssembly.Memory;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_5: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_export_7: WebAssembly.Table;
  readonly __externref_drop_slice: (a: number, b: number) => void;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly closure117337_externref_shim: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__convert__closures_____invoke__heac94dfe74335608: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h2ce973260553dde0: (a: number, b: number) => void;
  readonly closure373_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure114711_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure114714_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure117389_externref_shim: (a: number, b: number, c: any, d: any) => void;
  readonly __wbindgen_thread_destroy: (a?: number, b?: number, c?: number) => void;
  readonly __wbindgen_start: (a: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput, memory?: WebAssembly.Memory, thread_stack_size?: number }} module - Passing `SyncInitInput` directly is deprecated.
* @param {WebAssembly.Memory} memory - Deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput, memory?: WebAssembly.Memory, thread_stack_size?: number } | SyncInitInput, memory?: WebAssembly.Memory): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput>, memory?: WebAssembly.Memory, thread_stack_size?: number }} module_or_path - Passing `InitInput` directly is deprecated.
* @param {WebAssembly.Memory} memory - Deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput>, memory?: WebAssembly.Memory, thread_stack_size?: number } | InitInput | Promise<InitInput>, memory?: WebAssembly.Memory): Promise<InitOutput>;
