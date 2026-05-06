/* tslint:disable */
/* eslint-disable */

type DirEntryType = "file" | "directory";

/**
 * Options for the build operation, exposed to JS with auto-generated typings.
 */
export class BuildOptions {
    free(): void;
    [Symbol.dispose](): void;
    constructor();
    config: any;
    /**
     * When true, drops the existing global project and creates a fresh instance.
     */
    cleanup: boolean;
}

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

export function ERR_ABORT(): string;

export function ERR_INVALID_STATE(): string;

export function ERR_KEY_ALREADY_EXISTS(): string;

export function ERR_NOT_ALLOWED(): string;

export function ERR_NOT_FOUND(): string;

export function ERR_NO_MODIFICATION_ALLOWED(): string;

export function ERR_QUOTA_EXCEEDED(): string;

export function ERR_TYPE_MISMATCH(): string;

export class Fs {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    static copyFile(src: string, dst: string): Promise<void>;
    static copyFileSync(src: string, dst: string): void;
    static createDir(path: string): Promise<void>;
    static createDirAll(path: string): Promise<void>;
    static createDirAllSync(path: string): void;
    static createDirSync(path: string): void;
    static metadata(path: string): Promise<Metadata>;
    static metadataSync(path: string): Metadata;
    static read(path: string): Promise<Uint8Array>;
    static readDir(path: string): Promise<DirEntry[]>;
    static readDirSync(path: string): DirEntry[];
    static readSync(path: string): Uint8Array;
    static readToString(path: string): Promise<string>;
    static removeDir(path: string, recursive: boolean): Promise<void>;
    static removeDirSync(path: string, recursive: boolean): void;
    static removeFile(path: string): Promise<void>;
    static removeFileSync(path: string): void;
    static write(path: string, content: Uint8Array): Promise<void>;
    static writeString(path: string, content: string): Promise<void>;
    static writeSync(path: string, content: Uint8Array): void;
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
    file_size: bigint;
    type: DirEntryType;
}

export class Project {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    static build(options: BuildOptions): Promise<any>;
    /**
     * Generate package-lock.json by resolving dependencies.
     */
    static deps(registry?: string | null, concurrency?: number | null): Promise<string>;
    /**
     * Subscribe to entrypoints changes with HMR support.
     * This will watch for file changes and automatically rebuild.
     * Returns a RootTask that must be held by JS to keep the subscription active.
     */
    static entrypointsSubscribe(config: any, callback: Function): Promise<RootTask>;
    /**
     * Create a tar.gz archive and return bytes (no file I/O)
     */
    static gzip(files: any): Promise<Uint8Array>;
    /**
     * Subscribe to HMR events for a specific identifier.
     * Returns a RootTask that must be held by JS to keep the subscription active.
     */
    static hmrEvents(identifier: string, callback: Function): Promise<RootTask>;
    static init(thread_url: string): void;
    /**
     * Install dependencies - downloads tgz files only, extracts on-demand when files are read
     */
    static install(package_lock: string, max_concurrent_downloads?: number | null): Promise<void>;
    static setCwd(path: string): void;
    /**
     * Calculate MD5 hash of byte content (async for better thread scheduling)
     */
    static sigMd5(content: Uint8Array): Promise<string>;
    /**
     * Subscribe to compilation lifecycle events.
     * Emits "start" when computation begins, "end" when idle for aggregation_ms.
     */
    static updateInfoSubscribe(aggregation_ms: number, callback: Function): void;
    /**
     * Write all entrypoints to disk.
     */
    static writeAllToDisk(callback: Function): void;
    static readonly cwd: string;
}

/**
 * A root task handle that keeps the turbo-tasks subscription alive.
 * This must be held by JS to keep the subscription active.
 */
export class RootTask {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
}

export class WasmTaskMessage {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    taskId: number;
    readonly data: Uint8Array;
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
    cwd: string;
    filename: string;
}

export class WebWorkerTermination {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    options: WebWorkerOptions;
    workerId: number;
}

export function getWasmMemory(): any;

export function getWasmModule(): any;

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
    readonly initLogFilter: (a: number, b: number) => void;
    readonly init_pack: () => void;
    readonly __wbg_direntry_free: (a: number, b: number) => void;
    readonly __wbg_fs_free: (a: number, b: number) => void;
    readonly __wbg_get_direntry_name: (a: number) => [number, number];
    readonly __wbg_get_direntry_type: (a: number) => number;
    readonly __wbg_get_metadata_file_size: (a: number) => bigint;
    readonly __wbg_get_metadata_type: (a: number) => number;
    readonly __wbg_metadata_free: (a: number, b: number) => void;
    readonly __wbg_set_direntry_name: (a: number, b: number, c: number) => void;
    readonly __wbg_set_direntry_type: (a: number, b: number) => void;
    readonly __wbg_set_metadata_file_size: (a: number, b: bigint) => void;
    readonly __wbg_set_metadata_type: (a: number, b: number) => void;
    readonly fs_copyFile: (a: number, b: number, c: number, d: number) => any;
    readonly fs_copyFileSync: (a: number, b: number, c: number, d: number) => [number, number];
    readonly fs_createDir: (a: number, b: number) => any;
    readonly fs_createDirAll: (a: number, b: number) => any;
    readonly fs_createDirAllSync: (a: number, b: number) => [number, number];
    readonly fs_createDirSync: (a: number, b: number) => [number, number];
    readonly fs_metadata: (a: number, b: number) => any;
    readonly fs_metadataSync: (a: number, b: number) => [number, number, number];
    readonly fs_read: (a: number, b: number) => any;
    readonly fs_readDir: (a: number, b: number) => any;
    readonly fs_readDirSync: (a: number, b: number) => [number, number, number, number];
    readonly fs_readSync: (a: number, b: number) => [number, number, number];
    readonly fs_readToString: (a: number, b: number) => any;
    readonly fs_removeDir: (a: number, b: number, c: number) => any;
    readonly fs_removeDirSync: (a: number, b: number, c: number) => [number, number];
    readonly fs_removeFile: (a: number, b: number) => any;
    readonly fs_removeFileSync: (a: number, b: number) => [number, number];
    readonly fs_write: (a: number, b: number, c: any) => any;
    readonly fs_writeString: (a: number, b: number, c: number, d: number) => any;
    readonly fs_writeSync: (a: number, b: number, c: any) => [number, number];
    readonly __wbg_buildoptions_free: (a: number, b: number) => void;
    readonly __wbg_get_buildoptions_cleanup: (a: number) => number;
    readonly __wbg_roottask_free: (a: number, b: number) => void;
    readonly __wbg_set_buildoptions_cleanup: (a: number, b: number) => void;
    readonly buildoptions_config: (a: number) => any;
    readonly buildoptions_new: () => number;
    readonly buildoptions_set_config: (a: number, b: any) => void;
    readonly registerWorkerScheduler: (a: any, b: any) => void;
    readonly workerCreated: (a: number) => void;
    readonly __wbg_project_free: (a: number, b: number) => void;
    readonly project_build: (a: number) => any;
    readonly project_cwd: () => [number, number];
    readonly project_deps: (a: number, b: number, c: number) => any;
    readonly project_entrypointsSubscribe: (a: any, b: any) => any;
    readonly project_gzip: (a: any) => any;
    readonly project_hmrEvents: (a: number, b: number, c: any) => any;
    readonly project_init: (a: number, b: number) => void;
    readonly project_install: (a: number, b: number, c: number) => any;
    readonly project_setCwd: (a: number, b: number) => void;
    readonly project_sigMd5: (a: any) => any;
    readonly project_updateInfoSubscribe: (a: number, b: any) => void;
    readonly project_writeAllToDisk: (a: any) => void;
    readonly getWasmMemory: () => any;
    readonly getWasmModule: () => any;
    readonly ERR_ABORT: () => [number, number];
    readonly ERR_INVALID_STATE: () => [number, number];
    readonly ERR_KEY_ALREADY_EXISTS: () => [number, number];
    readonly ERR_NOT_ALLOWED: () => [number, number];
    readonly ERR_NOT_FOUND: () => [number, number];
    readonly ERR_NO_MODIFICATION_ALLOWED: () => [number, number];
    readonly ERR_QUOTA_EXCEEDED: () => [number, number];
    readonly ERR_TYPE_MISMATCH: () => [number, number];
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
    readonly wasm_bindgen_4920c120ac6c1288___closure__destroy___dyn_core_76fecec4534452f4___ops__function__Fn__js_sys_652c68a2be8008da___Array____Output_______: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___closure__destroy___dyn_core_76fecec4534452f4___ops__function__FnMut_____Output_______: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___closure__destroy___dyn_core_76fecec4534452f4___ops__function__FnMut__wasm_bindgen_4920c120ac6c1288___JsValue____Output_______: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___closure__destroy___dyn_core_76fecec4534452f4___ops__function__FnMut__web_sys_af693b13703c27a___features__gen_MessageEvent__MessageEvent____Output_______: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___closure__destroy___dyn_core_76fecec4534452f4___ops__function__FnMut_____Output________1_: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___closure__destroy___dyn_for__a__core_76fecec4534452f4___ops__function__FnMut____a_web_sys_af693b13703c27a___features__gen_MessageEvent__MessageEvent____Output_______: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures_____invoke___js_sys_652c68a2be8008da___Function__js_sys_652c68a2be8008da___Function_____: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures_____invoke___js_sys_652c68a2be8008da___Array_____: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures_____invoke___wasm_bindgen_4920c120ac6c1288___JsValue_____: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures_____invoke___web_sys_af693b13703c27a___features__gen_MessageEvent__MessageEvent_____: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures________invoke___web_sys_af693b13703c27a___features__gen_MessageEvent__MessageEvent_____: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures_____invoke______: (a: number, b: number) => void;
    readonly wasm_bindgen_4920c120ac6c1288___convert__closures_____invoke_______1_: (a: number, b: number) => void;
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
