import initWasm from "./utoo";

declare let self: WorkerGlobalScope;

(self as any).wasm_bindgen = initWasm;
