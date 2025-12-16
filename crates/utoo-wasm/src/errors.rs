use wasm_bindgen::prelude::*;

pub fn to_js_error<E>(e: E) -> JsError
where
    E: Into<anyhow::Error>,
{
    JsError::new(&format!("{:#?}", e.into()))
}
