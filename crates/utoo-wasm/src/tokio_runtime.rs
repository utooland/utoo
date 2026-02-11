use std::{
    cell::{OnceCell, RefCell},
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
};

use tokio::{
    runtime::{self, Runtime},
    time::Instant,
};

#[cfg(feature = "utoopack")]
thread_local! {
    static LAST_SWC_ATOM_GC_TIME: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

pub static TOKIO_RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Get the tokio runtime, panics if not initialized.
pub fn runtime() -> &'static Runtime {
    TOKIO_RUNTIME
        .get()
        .expect("tokio runtime not initialized, call init_tokio_runtime first")
}

pub fn init_tokio_runtime(worker_url: String) {
    TOKIO_RUNTIME.get_or_init(|| {
        let concurrency = (|| {
            let global = js_sys::global();
            let navigator =
                js_sys::Reflect::get(&global, &wasm_bindgen::JsValue::from_str("navigator"))
                    .ok()?;
            let concurrency = js_sys::Reflect::get(
                &navigator,
                &wasm_bindgen::JsValue::from_str("hardwareConcurrency"),
            )
            .ok()?;
            concurrency.as_f64().map(|v| (v as usize).max(1))
        })()
        .unwrap_or(1);

        let mut builder = runtime::Builder::new_multi_thread();
        builder
            .disable_lifo_slot()
            .max_blocking_threads(concurrency);

        #[cfg(feature = "utoopack")]
        builder
            .on_thread_stop(|| {
                turbo_tasks_malloc::TurboMalloc::thread_stop();
            })
            .on_thread_park(|| {
                LAST_SWC_ATOM_GC_TIME.with_borrow_mut(|cell| {
                    if cell.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                        swc_core::ecma::atoms::hstr::global_atom_store_gc();
                        *cell = Some(Instant::now());
                    }
                });
            });

        builder
            .wasm_bindgen_shim_url(worker_url.clone())
            .build()
            .expect("Failed to build tokio runtime")
    });
}
