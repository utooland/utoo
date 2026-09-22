//! Ownership and asynchronous draining of native background work.
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinHandle};

static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static IDLE: Notify = Notify::const_new();

/// Keep a resource-bearing worker registered until its actual completion.
#[must_use]
pub struct WorkGuard(());
impl WorkGuard {
    pub fn start() -> Self {
        ACTIVE.fetch_add(1, Ordering::AcqRel);
        Self(())
    }
}
impl Drop for WorkGuard {
    fn drop(&mut self) {
        if ACTIVE.fetch_sub(1, Ordering::AcqRel) == 1 {
            IDLE.notify_waiters();
        }
    }
}

/// Await all registered work after callers stop admitting new operations.
pub async fn wait_for_idle() {
    loop {
        let notified = IDLE.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ACTIVE.load(Ordering::Acquire) == 0 {
            return;
        }
        notified.await;
    }
}

struct Tracked<F> {
    future: Option<Pin<Box<F>>>,
    guard: Option<WorkGuard>,
}
impl<F: Future> Future for Tracked<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let result = this
            .future
            .as_mut()
            .expect("polled after completion")
            .as_mut()
            .poll(cx);
        if result.is_ready() {
            this.future.take();
            this.guard.take();
        }
        result
    }
}
impl<F> Drop for Tracked<F> {
    fn drop(&mut self) {
        // Future destruction may enqueue resource cleanup. Do that before
        // announcing idle, including when Tokio aborts or catches a panic.
        self.future.take();
        self.guard.take();
    }
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(Tracked {
        future: Some(Box::pin(future)),
        guard: Some(WorkGuard::start()),
    })
}

/// Blocking work cannot be aborted once started. Keep it registered until
/// the closure and its captured resources have actually been dropped.
pub fn spawn_blocking<F, R>(work: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let work = BlockingWork {
        work: Some(work),
        _guard: WorkGuard::start(),
    };
    tokio::task::spawn_blocking(move || work.run())
}

struct BlockingWork<F> {
    work: Option<F>,
    _guard: WorkGuard,
}
impl<F> BlockingWork<F> {
    fn run<R>(mut self) -> R
    where
        F: FnOnce() -> R,
    {
        self.work.take().expect("blocking work runs once")()
    }
}
impl<F> Drop for BlockingWork<F> {
    fn drop(&mut self) {
        // Tokio can discard an aborted job before its blocking closure starts.
        // Captured resources may enqueue cleanup in their destructors; keep
        // the guard alive through that destruction just as Tracked does.
        self.work.take();
    }
}

/// A join future which aborts its async task when the owner is cancelled.
/// Blocking/CPU work started by that task keeps its own WorkGuard.
pub struct OwnedTask<T>(JoinHandle<T>);
impl<T> Future for OwnedTask<T> {
    type Output = Result<T, JoinError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub fn spawn_owned<F>(future: F) -> OwnedTask<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    OwnedTask(spawn(future))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_queued_blocking_work_waits_for_resource_destruction() {
        // The idle counter is process-wide. Isolate this assertion from other
        // tests' background work, which could otherwise hide premature idle.
        const CHILD: &str = "UTOO_TEST_BLOCKING_CANCEL_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "util::task::tests::cancelling_queued_blocking_work_waits_for_resource_destruction",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                let (unblock_tx, unblock_rx) = std::sync::mpsc::channel();
                let occupying = tokio::task::spawn_blocking(move || {
                    started_tx.send(()).unwrap();
                    unblock_rx.recv().unwrap();
                });
                started_rx.await.unwrap();

                struct Resource {
                    dropping: Option<tokio::sync::oneshot::Sender<()>>,
                    release: std::sync::mpsc::Receiver<()>,
                }
                impl Drop for Resource {
                    fn drop(&mut self) {
                        let _ = self.dropping.take().unwrap().send(());
                        let _ = self.release.recv();
                    }
                }
                let (dropping_tx, dropping_rx) = tokio::sync::oneshot::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let resource = Resource {
                    dropping: Some(dropping_tx),
                    release: release_rx,
                };
                let queued = spawn_blocking(move || drop(resource));
                queued.abort();
                unblock_tx.send(()).unwrap();
                occupying.await.unwrap();
                dropping_rx.await.unwrap();

                let mut idle = Box::pin(wait_for_idle());
                assert!(futures::poll!(idle.as_mut()).is_pending());
                release_tx.send(()).unwrap();
                assert!(queued.await.unwrap_err().is_cancelled());
                tokio::time::timeout(std::time::Duration::from_secs(5), idle)
                    .await
                    .unwrap();
            })
            .await
            .expect("blocking cancellation did not complete");
        });
    }

    #[tokio::test]
    async fn dropping_an_owner_cancels_and_reaps_its_future() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        struct OnDrop(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let owned = spawn_owned(async move {
            let _drop = OnDrop(Some(dropped_tx));
            started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        drop(owned);
        dropped_rx.await.unwrap();
        wait_for_idle().await;
    }
}
