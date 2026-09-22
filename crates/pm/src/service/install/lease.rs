//! Keep a tool root exclusive through materialization, hooks and cancellation.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::util::process_lock::{ProcessLock, lock_exclusive, sibling_lock_path};
use anyhow::Result;

pub(super) struct InstallLease {
    root: PathBuf,
    lock: Option<ProcessLock>,
    cleanup: AtomicBool,
}

impl InstallLease {
    pub(super) async fn acquire(root: PathBuf, cleanup: bool) -> Result<Arc<Self>> {
        let lock = lock_exclusive(&sibling_lock_path(&root, ".install.lock")?).await?;
        Ok(Arc::new(Self {
            root,
            lock: Some(lock),
            cleanup: AtomicBool::new(cleanup),
        }))
    }

    pub(super) fn commit(&self) {
        self.cleanup.store(false, Ordering::Release);
    }
}

impl Drop for InstallLease {
    fn drop(&mut self) {
        if self.cleanup.load(Ordering::Acquire) {
            let lock = self.lock.take();
            let root = self.root.clone();
            // The last scheduler/child owner releases the lease. A new
            // preparation cannot acquire it until incomplete content is gone.
            utoo_ruborist::util::task::spawn_blocking(move || {
                let _lock = lock;
                if let Err(error) = std::fs::remove_dir_all(&root)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::debug!(
                        "Failed to clean incomplete tool at {}: {error}",
                        root.display()
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn incomplete_tool_is_cleaned_only_after_the_last_worker_releases_it() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("tool");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("partial"), "in progress").unwrap();
        let owner = InstallLease::acquire(root.clone(), true).await.unwrap();
        let worker = owner.clone();
        drop(owner);
        let (release, blocked) = std::sync::mpsc::channel();
        let task = utoo_ruborist::util::task::spawn_blocking(move || {
            let _worker = worker;
            blocked.recv().unwrap();
        });
        assert!(root.join("partial").exists());
        release.send(()).unwrap();
        task.await.unwrap();
        utoo_ruborist::util::task::wait_for_idle().await;
        assert!(!root.exists());
        let retry = InstallLease::acquire(root.clone(), true).await.unwrap();
        std::fs::create_dir(&root).unwrap();
        retry.commit();
        drop(retry);
        assert!(root.exists());
    }
}
