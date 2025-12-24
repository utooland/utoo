use std::{
    cell::OnceCell,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
};

use tokio::runtime;

pub static TOKIO_RUNTIME: OnceLock<runtime::Runtime> = OnceLock::new();

pub fn init_tokio_runtime(worker_url: String) {
    TOKIO_RUNTIME.get_or_init(|| {
        runtime::Builder::new_multi_thread()
            .disable_lifo_slot()
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(1);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-runtime-worker-{id}")
            })
            .wasm_bindgen_shim_url(worker_url.clone())
            .build()
            .unwrap()
    });
}
