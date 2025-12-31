/* tslint:disable */
/* eslint-disable */

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
  type: DirEntryType;
  file_size: bigint;
}

export class Project {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  static createDir(path: string): Promise<void>;
  static removeDir(path: string, recursive: boolean): Promise<void>;
  static writeSync(path: string, content: Uint8Array): void;
  static removeFile(path: string): Promise<void>;
  static writeString(path: string, content: string): Promise<void>;
  static metadataSync(path: string): Metadata;
  static readDirSync(path: string): DirEntry[];
  static copyFileSync(src: string, dst: string): void;
  static createDirAll(path: string): Promise<void>;
  static readToString(path: string): Promise<string>;
  static createDirSync(path: string): void;
  static removeDirSync(path: string, recursive: boolean): void;
  static removeFileSync(path: string): void;
  static createDirAllSync(path: string): void;
  /**
   * Generate package-lock.json by resolving dependencies.
   *
   * # Arguments
   * * `registry` - Optional registry URL. If None, uses npmmirror.
   *   - "https://registry.npmmirror.com" - supports semver queries (faster)
   *   - "https://registry.npmjs.org" - official npm registry (slower, fetches full manifest)
   * * `concurrency` - Optional concurrency limit (defaults to 20)
   */
  static deps(registry?: string | null, concurrency?: number | null): Promise<string>;
  /**
   * Create a tar.gz archive and return bytes (no file I/O)
   * This is useful for main thread execution without OPFS access
   */
  static gzip(files: any): Promise<Uint8Array>;
  static init(thread_url: string): void;
  static read(path: string): Promise<Uint8Array>;
  static build(): Promise<any>;
  static write(path: string, content: Uint8Array): Promise<void>;
  static install(package_lock: string, max_concurrent_downloads?: number | null): Promise<void>;
  static setCwd(path: string): void;
  /**
   * Calculate MD5 hash of byte content (async for better thread scheduling)
   */
  static sigMd5(content: Uint8Array): Promise<string>;
  static metadata(path: string): Promise<Metadata>;
  static readDir(path: string): Promise<DirEntry[]>;
  static copyFile(src: string, dst: string): Promise<void>;
  static readSync(path: string): Uint8Array;
  static readonly cwd: string;
}

export class WasmTaskMessage {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  readonly data: Uint8Array;
  taskId: number;
}

export class WebWorkerCreation {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  options: WebWorkerOptions;
}

export class WebWorkerOptions {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  filename: string;
  cwd: string;
}

export class WebWorkerTermination {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  options: WebWorkerOptions;
  workerId: number;
}

export function initLogFilter(filter: string): void;

export function init_pack(): void;

export function recvTaskMessageInWorker(worker_id: number): Promise<WasmTaskMessage>;

export function registerWorkerScheduler(creator: Function, terminator: Function): void;

export function sendTaskMessage(message: any): Promise<void>;

/**
 * Entry point for web workers
 */
export function wasm_thread_entry_point(ptr: number): void;

export function workerCreated(worker_id: number): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly registerWorkerScheduler: (a: any, b: any) => void;
  readonly workerCreated: (a: number) => void;
  readonly initLogFilter: (a: number, b: number) => void;
  readonly init_pack: () => void;
  readonly __wbg_direntry_free: (a: number, b: number) => void;
  readonly __wbg_get_direntry_name: (a: number) => [number, number];
  readonly __wbg_get_direntry_type: (a: number) => number;
  readonly __wbg_get_metadata_file_size: (a: number) => bigint;
  readonly __wbg_get_metadata_type: (a: number) => number;
  readonly __wbg_metadata_free: (a: number, b: number) => void;
  readonly __wbg_project_free: (a: number, b: number) => void;
  readonly __wbg_set_direntry_name: (a: number, b: number, c: number) => void;
  readonly __wbg_set_direntry_type: (a: number, b: number) => void;
  readonly __wbg_set_metadata_file_size: (a: number, b: bigint) => void;
  readonly __wbg_set_metadata_type: (a: number, b: number) => void;
  readonly project_build: () => any;
  readonly project_copyFile: (a: number, b: number, c: number, d: number) => any;
  readonly project_copyFileSync: (a: number, b: number, c: number, d: number) => [number, number];
  readonly project_createDir: (a: number, b: number) => any;
  readonly project_createDirAll: (a: number, b: number) => any;
  readonly project_createDirAllSync: (a: number, b: number) => [number, number];
  readonly project_createDirSync: (a: number, b: number) => [number, number];
  readonly project_cwd: () => [number, number];
  readonly project_deps: (a: number, b: number, c: number) => any;
  readonly project_gzip: (a: any) => any;
  readonly project_init: (a: number, b: number) => void;
  readonly project_install: (a: number, b: number, c: number) => any;
  readonly project_metadata: (a: number, b: number) => any;
  readonly project_metadataSync: (a: number, b: number) => [number, number, number];
  readonly project_read: (a: number, b: number) => any;
  readonly project_readDir: (a: number, b: number) => any;
  readonly project_readDirSync: (a: number, b: number) => [number, number, number, number];
  readonly project_readSync: (a: number, b: number) => [number, number, number];
  readonly project_readToString: (a: number, b: number) => any;
  readonly project_removeDir: (a: number, b: number, c: number) => any;
  readonly project_removeDirSync: (a: number, b: number, c: number) => [number, number];
  readonly project_removeFile: (a: number, b: number) => any;
  readonly project_removeFileSync: (a: number, b: number) => [number, number];
  readonly project_setCwd: (a: number, b: number) => void;
  readonly project_sigMd5: (a: any) => any;
  readonly project_write: (a: number, b: number, c: any) => any;
  readonly project_writeString: (a: number, b: number, c: number, d: number) => any;
  readonly project_writeSync: (a: number, b: number, c: any) => [number, number];
  readonly rust_mi_get_default_heap: () => number;
  readonly rust_mi_get_thread_id: () => number;
  readonly rust_mi_set_default_heap: (a: number) => void;
  readonly __wbg_get_wasmtaskmessage_taskId: (a: number) => number;
  readonly __wbg_get_webworkercreation_options: (a: number) => number;
  readonly __wbg_get_webworkeroptions_cwd: (a: number) => [number, number];
  readonly __wbg_get_webworkeroptions_filename: (a: number) => [number, number];
  readonly __wbg_get_webworkertermination_options: (a: number) => number;
  readonly __wbg_get_webworkertermination_workerId: (a: number) => number;
  readonly __wbg_set_wasmtaskmessage_taskId: (a: number, b: number) => void;
  readonly __wbg_set_webworkercreation_options: (a: number, b: number) => void;
  readonly __wbg_set_webworkeroptions_cwd: (a: number, b: number, c: number) => void;
  readonly __wbg_set_webworkeroptions_filename: (a: number, b: number, c: number) => void;
  readonly __wbg_set_webworkertermination_options: (a: number, b: number) => void;
  readonly __wbg_set_webworkertermination_workerId: (a: number, b: number) => void;
  readonly __wbg_wasmtaskmessage_free: (a: number, b: number) => void;
  readonly __wbg_webworkercreation_free: (a: number, b: number) => void;
  readonly __wbg_webworkeroptions_free: (a: number, b: number) => void;
  readonly __wbg_webworkertermination_free: (a: number, b: number) => void;
  readonly recvTaskMessageInWorker: (a: number) => any;
  readonly sendTaskMessage: (a: any) => any;
  readonly wasmtaskmessage_data: (a: number) => any;
  readonly __wbg_createsyncaccesshandleoptions_free: (a: number, b: number) => void;
  readonly wasm_thread_entry_point: (a: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__hce460cf0a813c454: (a: number, b: number) => void;
  readonly wasm_bindgen__closure__destroy__hc14e8607e23cebd7: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h05951ccee93604a9: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__hc95bb6053e4d238d: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures________invoke__hc572a1bf691b2f02: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__h79b136ee1664aaaa: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h12ad3f2e11ef8f1f: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__ha0c175f92395ef9a: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h1fc3e28920f04101: (a: number, b: number) => void;
  readonly wasm_bindgen__closure__destroy__h697219d9b2418ea3: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h8bdf724950a20526: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__h33b24a5c45343d26: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__heb9448f3005917b6: (a: number, b: number, c: any, d: any) => void;
  readonly memory: WebAssembly.Memory;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __externref_drop_slice: (a: number, b: number) => void;
  readonly __externref_table_dealloc: (a: number) => void;
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
