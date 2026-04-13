import { initSync } from "../utoo";

declare let self: WorkerGlobalScope;

// this is for wasm_thread to spawn new worker thread.
// https://github.com/utooland/wasm_thread/blob/fd53c2f5a20e9f899d9216dee045472f50d97219/src/wasm32/js/web_worker.sync.js#L10
(self as any).wasm_bindgen = initSync;
