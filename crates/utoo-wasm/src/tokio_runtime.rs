use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        LazyLock,
    },
    time::Duration,
};

use tokio::runtime;

pub static TOKIO_RUNTIME: LazyLock<runtime::Runtime> = LazyLock::new(|| {
    runtime::Builder::new_multi_thread()
        // .enable_time()
        // .disable_lifo_slot()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("tokio-runtime-worker-{id}")
        })
        .wasm_bindgen_shim_url("http://localhost:8080/tokio_worker.js".to_string())
        .build()
        .unwrap()
});
