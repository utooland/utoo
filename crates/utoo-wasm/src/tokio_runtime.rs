use std::{
    cell::OnceCell,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
};

use tokio::runtime::{self, Runtime};

pub static TOKIO_RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Get the tokio runtime, panics if not initialized.
pub fn runtime() -> &'static Runtime {
    TOKIO_RUNTIME
        .get()
        .expect("tokio runtime not initialized, call init_tokio_runtime first")
}

pub fn init_tokio_runtime(worker_url: String) {
    TOKIO_RUNTIME.get_or_init(|| {
        runtime::Builder::new_multi_thread()
            .disable_lifo_slot()
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(1);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-runtime-worker-{id}")
            })
            .max_blocking_threads(1)
            .wasm_bindgen_shim_url(worker_url.clone())
            .build()
            .expect("Failed to build tokio runtime")
    });
}
