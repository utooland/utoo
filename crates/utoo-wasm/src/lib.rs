#![cfg(all(target_family = "wasm", target_os = "unknown"))]

extern crate console_error_panic_hook;

use std::panic;

use pack_core::tracing_presets::TRACING_OVERVIEW_TARGETS;
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

mod opfs_offload;
mod project;
pub(crate) mod tokio_runtime;
pub use project::Project;

#[wasm_bindgen(start)]
#[cfg(feature = "utoo-pack")]
fn init_pack() {
    use crate::opfs_offload::OpfsOffload;

    panic::set_hook(Box::new(console_error_panic_hook::hook));

    let fmt_layer = fmt::layer()
        .without_time()
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(MakeWebConsoleWriter::new())
        .with_filter(EnvFilter::new({
            let mut trace = TRACING_OVERVIEW_TARGETS
                .iter()
                // .filter(|_| true)
                .cloned()
                .collect::<Vec<&str>>();

            trace.extend(["utoo_wasm=info"]);
            trace.join(",")
        }));

    registry().with(fmt_layer).init();

    #[cfg(feature = "utoo-pack")]
    pack::register();

    wasm_bindgen_futures::spawn_local(turbo_tasks_fs::wasm_fs_offload::server(OpfsOffload))
}
