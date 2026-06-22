//! Off-runtime CPU dispatch.

/// Run a CPU-bound closure on rayon's thread pool (native) or inline
/// (wasm32), awaiting the result without blocking the tokio runtime.
///
/// On native this keeps `simd_json` / manifest re-parsing off the async
/// executor so sibling network fetches keep driving IO while this one
/// computes. The closure must be `'static` because rayon owns it for the
/// duration of the spawned task. Panics if the worker is dropped before
/// sending — that only happens when the closure itself panics, which is a
/// bug worth surfacing loudly.
pub(crate) async fn spawn_cpu<R, F>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        rayon::spawn(move || {
            let _ = tx.send(f());
        });
        rx.await.expect("rayon cpu worker dropped before sending")
    }
    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}
