#![cfg(all(target_family = "wasm", target_os = "unknown"))]

extern crate console_error_panic_hook;

use std::panic;

use pack_core::tracing_presets::TRACING_TURBO_TASKS_TARGETS;
use tracing_subscriber::{
    fmt::{self, format::Pretty},
    layer::SubscriberExt,
    registry,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

use tracing_web::{performance_layer, MakeWebConsoleWriter};
use wasm_bindgen::prelude::wasm_bindgen;

pub(crate) mod pack;
mod project;

pub use project::Project;

#[wasm_bindgen(start)]
fn init_pack() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    let fmt_layer = fmt::layer()
        .without_time()
        .with_writer(MakeWebConsoleWriter::new())
        .with_filter(EnvFilter::new({
            let mut trace = TRACING_TURBO_TASKS_TARGETS
                .iter()
                // .filter(|_| true)
                .cloned()
                .collect::<Vec<&str>>();
            trace.push("utoo_wasm");
            trace.join(",")
        }));

    registry()
        .with(fmt_layer)
        // .with(performance_layer().with_details_from_fields(Pretty::default()))
        .init();

    pack::register();
}
