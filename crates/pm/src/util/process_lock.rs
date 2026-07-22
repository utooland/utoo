use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// An exclusive OS lock held for as long as the backing file handle is open.
///
/// The lock file is intentionally retained after release. Removing it could
/// split waiters and new contenders across different filesystem objects.
pub struct ProcessLock {
    _file: File,
}

fn open_lock_file(lock_path: &Path) -> Result<File> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create lock parent directory {}",
                parent.display()
            )
        })?;
    }

    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .with_context(|| format!("Failed to open lock file {}", lock_path.display()))
}

/// Acquire an exclusive process lock without blocking an async executor thread.
pub async fn lock_exclusive(lock_path: &Path) -> Result<ProcessLock> {
    let lock_path = lock_path.to_path_buf();
    tokio::task::spawn_blocking(move || lock_exclusive_sync(&lock_path))
        .await
        .context("Lock task failed")?
}

/// Acquire an exclusive process lock from an existing blocking worker.
pub fn lock_exclusive_sync(lock_path: &Path) -> Result<ProcessLock> {
    let file = open_lock_file(lock_path)?;
    file.lock()
        .with_context(|| format!("Failed to acquire lock on {}", lock_path.display()))?;
    Ok(ProcessLock { _file: file })
}

/// Return a stable sibling lock path outside the directory being protected.
///
/// `/cache/tapable/2.3.3` with suffix `.lock` becomes
/// `/cache/tapable/.2.3.3.lock`.
pub fn sibling_lock_path(path: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .with_context(|| format!("Lock target has no file name: {}", path.display()))?;
    let mut lock_name = OsString::from(".");
    lock_name.push(file_name);
    lock_name.push(suffix);
    Ok(path.with_file_name(lock_name))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    use tempfile::TempDir;

    use super::*;

    const CHILD_LOCK_PATH: &str = "UTOO_PROCESS_LOCK_TEST_PATH";
    const CHILD_STARTED_PATH: &str = "UTOO_PROCESS_LOCK_TEST_STARTED";
    const CHILD_ACQUIRED_PATH: &str = "UTOO_PROCESS_LOCK_TEST_ACQUIRED";

    #[test]
    fn sibling_path_keeps_lock_outside_protected_directory() {
        let target = Path::new("cache/tapable/2.3.3");
        assert_eq!(
            sibling_lock_path(target, ".lock").unwrap(),
            Path::new("cache/tapable/.2.3.3.lock")
        );
    }

    #[test]
    fn exclusive_lock_serializes_processes_and_keeps_empty_file() {
        if let Some(lock_path) = std::env::var_os(CHILD_LOCK_PATH) {
            let started_path = PathBuf::from(std::env::var_os(CHILD_STARTED_PATH).unwrap());
            let acquired_path = PathBuf::from(std::env::var_os(CHILD_ACQUIRED_PATH).unwrap());
            std::fs::write(started_path, b"").unwrap();
            let _lock = lock_exclusive_sync(Path::new(&lock_path)).unwrap();
            std::fs::write(acquired_path, b"").unwrap();
            return;
        }

        let temp = TempDir::new().unwrap();
        let target = temp.path().join("tapable/2.3.3");
        let lock_path = sibling_lock_path(&target, ".lock").unwrap();
        let started_path = temp.path().join("child-started");
        let acquired_path = temp.path().join("child-acquired");
        let parent_lock = lock_exclusive_sync(&lock_path).unwrap();

        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("exclusive_lock_serializes_processes_and_keeps_empty_file")
            .arg("--nocapture")
            .env(CHILD_LOCK_PATH, &lock_path)
            .env(CHILD_STARTED_PATH, &started_path)
            .env(CHILD_ACQUIRED_PATH, &acquired_path)
            .spawn()
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !started_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !started_path.exists() {
            drop(parent_lock);
            let _ = child.kill();
            let _ = child.wait();
            panic!("child process did not reach the lock attempt");
        }

        thread::sleep(Duration::from_millis(100));
        assert!(
            !acquired_path.exists(),
            "child acquired an exclusive lock while the parent held it"
        );

        drop(parent_lock);
        assert!(child.wait().unwrap().success());
        assert!(acquired_path.exists());
        assert_eq!(std::fs::metadata(lock_path).unwrap().len(), 0);
    }
}
