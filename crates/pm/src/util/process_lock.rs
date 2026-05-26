use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::Duration;

use anyhow::{Context, Result};

#[cfg(unix)]
pub struct ProcessLock {
    file: std::fs::File,
}

#[cfg(windows)]
pub struct ProcessLock {
    lock_dir: PathBuf,
}

/// Acquire a best-effort cross-process exclusive lock.
///
/// Unix uses `flock`, where stale locks are released by the kernel when the
/// owning process exits. The lock file is intentionally left on disk: deleting
/// it while another process is waiting can split the lock across two inodes.
#[cfg(unix)]
pub async fn lock_exclusive(lock_path: &Path) -> Result<ProcessLock> {
    use std::os::unix::io::AsRawFd;

    let lock_path = lock_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create lock parent directory {}",
                    parent.display()
                )
            })?;
        }

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("Failed to open lock file {}", lock_path.display()))?;

        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            anyhow::bail!("Failed to acquire lock on {}: {}", lock_path.display(), err);
        }

        Ok(ProcessLock { file })
    })
    .await?
}

#[cfg(unix)]
impl Drop for ProcessLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;

        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Windows fallback: an atomic lock directory with stale-lock recovery.
///
/// This avoids an unbounded hang if a process crashes while holding the lock.
/// It is less perfect than kernel-backed locking but keeps the install path
/// recoverable on platforms where this crate has no Windows locking dependency.
#[cfg(windows)]
pub async fn lock_exclusive(lock_path: &Path) -> Result<ProcessLock> {
    let lock_dir = lock_path.with_extension("lockdir");
    let stale_after = Duration::from_secs(30 * 60);
    if let Some(parent) = lock_dir.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "Failed to create lock parent directory {}",
                parent.display()
            )
        })?;
    }

    loop {
        match tokio::fs::create_dir(&lock_dir).await {
            Ok(()) => return Ok(ProcessLock { lock_dir }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Ok(meta) = tokio::fs::metadata(&lock_dir).await
                    && let Ok(modified) = meta.modified()
                    && modified.elapsed().unwrap_or_default() > stale_after
                {
                    let _ = tokio::fs::remove_dir_all(&lock_dir).await;
                    continue;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to create lock directory {}: {}",
                    lock_dir.display(),
                    e
                ));
            }
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.lock_dir);
    }
}

pub fn lock_path_for(path: &Path, suffix: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| {
            let mut name = name.to_os_string();
            name.push(suffix);
            name
        })
        .unwrap_or_else(|| suffix.into());

    path.with_file_name(file_name)
}
