use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;

#[wasm_bindgen(js_name = "getWasmModule")]
pub fn get_wasm_module() -> JsValue {
    wasm_bindgen::module()
}

#[wasm_bindgen(js_name = "getWasmMemory")]
pub fn get_wasm_memory() -> JsValue {
    wasm_bindgen::memory()
}
