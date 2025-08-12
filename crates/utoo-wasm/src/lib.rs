#![cfg(all(target_family = "wasm", target_os = "unknown"))]

use wasm_bindgen::prelude::wasm_bindgen;

pub(crate) mod pack;
mod project;

pub use project::Project;

#[wasm_bindgen(start)]
fn init_pack() {
    pack::register();
}
