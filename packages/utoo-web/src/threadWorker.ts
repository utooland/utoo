import initWasm from "./utoo";

declare let self: WorkerGlobalScope;

// this is for wasm_thread to spawn new worker thread.
// see: https://github.com/utooland/wasm_thread/blob/94438ff771ee0a6a55d79e49a655707970acb615/src/wasm32/js/web_worker.js#L10
(self as any).wasm_bindgen = initWasm;
