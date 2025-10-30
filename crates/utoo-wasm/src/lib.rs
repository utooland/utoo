#![cfg(all(target_family = "wasm", target_os = "unknown"))]

extern crate console_error_panic_hook;

use std::panic;

use tracing_subscriber::{
    fmt::{
        self,
        format::{FmtSpan, Pretty},
    },
    layer::SubscriberExt,
    registry,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

use tracing_web::{performance_layer, MakeWebConsoleWriter};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;

#[cfg(feature = "utoo-pack")]
pub(crate) mod pack;

#[cfg(feature = "utoo-pack")]
mod opfs_offload;
mod project;
pub(crate) mod tokio_runtime;
pub use project::Project;

#[wasm_bindgen(start)]
fn init_pack() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    let log_filter = get_global_log_filter().unwrap_or_else(|| {
        vec!["pack_core=info", "pack_api=info", "utoo_wasm=info"].join(",")
    });

    let fmt_layer = fmt::layer()
        .without_time()
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(MakeWebConsoleWriter::new())
        .with_filter(EnvFilter::new(log_filter));

    registry().with(fmt_layer).init();

    #[cfg(feature = "utoo-pack")]
    {
        unsafe {
            pack::__wasm_call_ctors();
        }
        wasm_bindgen_futures::spawn_local(turbo_tasks_fs::wasm_fs_offload::server(
            crate::opfs_offload::OpfsOffload,
        ))
    }
}

// get the global log filter from `globalThis.__UTOO_LOG_FILTER__`
fn get_global_log_filter() -> Option<String> {
    let global = js_sys::global();
    let key = JsValue::from_str("__UTOO_LOG_FILTER__");

    match js_sys::Reflect::get(&global, &key) {
        Ok(value) if !value.is_undefined() && !value.is_null() => {
            value.as_string()
        }
        _ => None,
    }
}
