#![cfg(all(target_family = "wasm", target_os = "unknown"))]

extern crate console_error_panic_hook;

use std::panic;

use std::sync::Once;
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

static TRACING_INIT: Once = Once::new();

#[cfg(feature = "utoopack")]
pub(crate) mod pack;

mod deps;
pub(crate) mod errors;
mod fs;
mod operations;
#[cfg(feature = "utoopack")]
mod opfs_offload;
mod pm;
mod project;
mod wasm_shim;

pub use fs::{DirEntry, DirEntryType, Fs, Metadata, OpfsGlob};
pub(crate) mod tokio_runtime;
pub use project::Project;
pub use wasm_shim::{get_wasm_memory, get_wasm_module};

#[global_allocator]
static ALLOC: turbo_tasks_malloc::TurboMalloc = turbo_tasks_malloc::TurboMalloc;

#[wasm_bindgen(start)]
fn init_pack() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    #[cfg(feature = "utoopack")]
    {
        unsafe {
            pack::__wasm_call_ctors();
        }
        wasm_bindgen_futures::spawn_local(turbo_tasks_fs::wasm_fs_offload::server(
            crate::opfs_offload::OpfsOffload,
        ))
    }
}

#[wasm_bindgen(js_name = "initLogFilter")]
pub fn init_log_filter(mut filter: String) {
    const DEFAULT_LOG_FILTER: &str = "utoo_wasm=info,pack_api=info,pack_core=info";

    if filter.is_empty() {
        filter.push_str(DEFAULT_LOG_FILTER)
    }
    TRACING_INIT.call_once(|| {
        let filter_str = filter.clone();
        let fmt_layer = fmt::layer()
            .without_time()
            .with_span_events(fmt::format::FmtSpan::NONE)
            // .with_span_events(fmt::format::FmtSpan::CLOSE)
            .with_writer(MakeWebConsoleWriter::new())
            .with_filter(EnvFilter::new(filter));

        registry().with(fmt_layer).init();
    });
}
