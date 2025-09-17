use std::{
    cell::OnceCell,
    sync::{
        atomic::{AtomicUsize, Ordering},
        LazyLock,
    },
    time::Duration,
};

use tokio::runtime;

thread_local! {
    pub static TOKIO_RUNTIME: OnceCell<runtime::Runtime> = const { OnceCell::new() };
}

pub fn init_tokio_runtime(worker_url: String) {
    TOKIO_RUNTIME.with(|ctx| {
        ctx.get_or_init(|| {
            runtime::Builder::new_multi_thread()
                /* *
                 * The default dlmalloc is a single global allocator,
                 * it will block threads scheduling,
                 * see https://web.dev/articles/scaling-multithreaded-webassembly-applications#heap_management_mallocfree
                 */
                .worker_threads(4)
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
    })
}
