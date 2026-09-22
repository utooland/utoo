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
pub async fn spawn_cpu<R, F>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let guard = super::task::WorkGuard::start();
        rayon::spawn(move || {
            let _guard = guard;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let _ = tx.send(result);
        });
        match rx.await.expect("rayon cpu worker dropped before sending") {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[tokio::test]
    async fn cpu_panic_returns_to_the_owner_without_killing_the_pool() {
        let panic = std::panic::AssertUnwindSafe(spawn_cpu(|| panic!("worker failure")))
            .catch_unwind()
            .await;
        assert!(panic.is_err());
        assert_eq!(spawn_cpu(|| 42).await, 42);
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_release_running_worker_resources() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = tokio::spawn(spawn_cpu(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        }));
        started_rx.await.unwrap();
        worker.abort();
        let _ = worker.await;
        let idle = super::super::task::wait_for_idle();
        tokio::pin!(idle);
        assert!(futures::poll!(&mut idle).is_pending());
        release_tx.send(()).unwrap();
        idle.await;
    }
}
