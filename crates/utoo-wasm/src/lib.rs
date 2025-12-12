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

#[cfg(feature = "utoo-pack")]
pub(crate) mod pack;

#[cfg(feature = "utoo-pack")]
mod opfs_offload;
mod project;
pub(crate) mod tokio_runtime;
pub use project::Project;

#[global_allocator]
static ALLOC: turbo_tasks_malloc::TurboMalloc = turbo_tasks_malloc::TurboMalloc;

#[wasm_bindgen(start)]
fn init_pack() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

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

#[wasm_bindgen]
pub fn init_log_filter(filter: String) {
    let fmt_layer = fmt::layer()
        .without_time()
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(MakeWebConsoleWriter::new())
        .with_filter(EnvFilter::new(filter));

    registry().with(fmt_layer).init();
}
