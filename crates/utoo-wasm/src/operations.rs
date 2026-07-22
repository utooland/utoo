use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        LazyLock,
    },
};

use futures::future::{self, Aborted};
use parking_lot::Mutex;
use tokio::{sync::Notify, task::JoinHandle};

#[derive(Clone)]
enum OperationAbortHandle {
    Local(future::AbortHandle),
    Tokio(tokio::task::AbortHandle),
}

impl OperationAbortHandle {
    fn abort(&self) {
        match self {
            Self::Local(handle) => handle.abort(),
            Self::Tokio(handle) => handle.abort(),
        }
    }
}

static ACTIVE_OPERATIONS: LazyLock<Mutex<HashMap<usize, OperationAbortHandle>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_OPERATION_ID: AtomicUsize = AtomicUsize::new(0);
static DISPOSING: AtomicBool = AtomicBool::new(false);
static OPERATIONS_CHANGED: LazyLock<Notify> = LazyLock::new(Notify::new);

pub(crate) struct OperationGuard {
    id: usize,
}

impl OperationGuard {
    pub(crate) fn track_tokio<T>(task: &JoinHandle<T>) -> Self {
        Self::track(OperationAbortHandle::Tokio(task.abort_handle()))
    }

    fn track(abort_handle: OperationAbortHandle) -> Self {
        let id = NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed);

        ACTIVE_OPERATIONS.lock().insert(id, abort_handle.clone());
        if DISPOSING.load(Ordering::Acquire) {
            abort_handle.abort();
        }

        Self { id }
    }
}

pub(crate) async fn run_local<F>(operation: F) -> Result<F::Output, Aborted>
where
    F: Future,
{
    let (operation, abort_handle) = future::abortable(operation);
    let _guard = OperationGuard::track(OperationAbortHandle::Local(abort_handle));
    operation.await
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        ACTIVE_OPERATIONS.lock().remove(&self.id);
        OPERATIONS_CHANGED.notify_one();
    }
}

pub(crate) fn reset() {
    DISPOSING.store(false, Ordering::Release);
}

pub(crate) async fn cancel_all() {
    DISPOSING.store(true, Ordering::Release);

    loop {
        let changed = OPERATIONS_CHANGED.notified();
        let abort_handles = {
            let operations = ACTIVE_OPERATIONS.lock();
            if operations.is_empty() {
                return;
            }
            operations.values().cloned().collect::<Vec<_>>()
        };

        for abort_handle in abort_handles {
            abort_handle.abort();
        }
        changed.await;
    }
}
