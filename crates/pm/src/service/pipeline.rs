//! Pipeline installer for concurrent manifest resolution and tarball downloading.
//!
//! This module implements a pipeline architecture similar to bun's approach:
//! - Manifest fetching and tarball downloading happen concurrently
//! - When a package is resolved, its tarball download starts immediately
//! - Uses global OnceMap to deduplicate requests and share results across phases

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use once_cell::sync::Lazy;
use tokio::sync::{Semaphore, mpsc};
use utoo_ruborist::progress::{BuildEvent, EventReceiver, PackageTarballInfo};

use crate::util::cache::get_cache_dir;
use crate::util::config::get_manifests_concurrency_limit_sync;
use crate::util::downloader::{download_bytes, extract_and_write};
use crate::util::oncemap::OnceMap;

// ============ Pipeline Statistics ============

/// Statistics for pipeline stages
pub struct PipelineStats {
    /// Total downloads completed
    pub downloaded: AtomicUsize,
    /// Total clones completed
    pub cloned: AtomicUsize,
}

impl PipelineStats {
    pub const fn new() -> Self {
        Self {
            downloaded: AtomicUsize::new(0),
            cloned: AtomicUsize::new(0),
        }
    }

    pub fn downloaded(&self) -> usize {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn cloned(&self) -> usize {
        self.cloned.load(Ordering::Relaxed)
    }

    /// Print summary
    pub fn print_summary(&self) {
        tracing::info!(
            "Pipeline stats: downloaded={}, cloned={}",
            self.downloaded(),
            self.cloned()
        );
    }
}

/// Global pipeline statistics
pub static STATS: PipelineStats = PipelineStats::new();

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path
static DOWNLOAD_CACHE: Lazy<OnceMap<String, PathBuf>> = Lazy::new(OnceMap::new);

/// Global clone cache shared between pipeline and install phases.
/// Key: target path, Value: ()
static CLONE_CACHE: Lazy<OnceMap<String, ()>> = Lazy::new(OnceMap::new);

/// Global semaphore for download concurrency control
static DOWNLOAD_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Global semaphore for clone concurrency control
static CLONE_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_download_semaphore() -> &'static Arc<Semaphore> {
    DOWNLOAD_SEMAPHORE.get_or_init(|| {
        let limit = get_manifests_concurrency_limit_sync();
        tracing::debug!("Initializing download semaphore with limit: {}", limit);
        Arc::new(Semaphore::new(limit))
    })
}

fn get_clone_semaphore() -> &'static Arc<Semaphore> {
    CLONE_SEMAPHORE.get_or_init(|| {
        let limit = get_manifests_concurrency_limit_sync();
        tracing::debug!("Initializing clone semaphore with limit: {}", limit);
        Arc::new(Semaphore::new(limit))
    })
}

/// Owned version of PackageTarballInfo for channel transmission
#[derive(Debug, Clone)]
pub struct OwnedPackageInfo {
    pub name: String,
    pub version: String,
    pub tarball_url: Option<String>,
}

impl OwnedPackageInfo {
    pub fn from_tarball_info(info: &PackageTarballInfo<'_>) -> Self {
        Self {
            name: info.name.to_string(),
            version: info.version.to_string(),
            tarball_url: info.tarball_url.map(|s| s.to_string()),
        }
    }
}

/// Message for clone worker
#[derive(Debug)]
pub struct CloneMessage {
    pub info: OwnedPackageInfo,
    pub path: PathBuf,
    /// Parent package path (None for root-level dependencies)
    pub parent_path: Option<PathBuf>,
}

/// Download a package tarball, using global OnceMap for deduplication.
pub async fn download_package(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    let key = format!("{}@{}", name, version);
    let cache_dir = get_cache_dir();
    let name = name.to_string();
    let version = version.to_string();
    let tarball_url = tarball_url.to_string();

    DOWNLOAD_CACHE
        .get_or_init(key, || async move {
            let cache_path = cache_dir.join(&name).join(&version);

            // Fast path: already extracted in cache
            if crate::fs::try_exists(&cache_path.join("_resolved"))
                .await
                .unwrap_or(false)
            {
                tracing::debug!("Cache hit: {}@{}", name, version);
                return Some(cache_path);
            }

            // Download (semaphore controlled)
            let _permit = get_download_semaphore().acquire().await.ok()?;
            let bytes = download_bytes(&tarball_url)
                .await
                .inspect_err(|e| tracing::warn!("Download failed: {}@{}: {}", name, version, e))
                .ok()?;

            // Extract
            extract_and_write(bytes, &cache_path)
                .await
                .inspect_err(|e| tracing::warn!("Extract failed: {}@{}: {}", name, version, e))
                .ok()?;

            STATS.downloaded.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Extracted: {}@{}", name, version);
            Some(cache_path)
        })
        .await
        .map(|arc| (*arc).clone())
}

/// Clone a package to target path, using global OnceMap for deduplication.
pub async fn clone_package_once(
    name: &str,
    version: &str,
    tarball_url: &str,
    target_path: &std::path::Path,
) -> bool {
    use crate::util::cloner::clone_package;

    let key = target_path.to_string_lossy().to_string();
    let name = name.to_string();
    let version = version.to_string();
    let tarball_url = tarball_url.to_string();
    let target_path = target_path.to_path_buf();

    CLONE_CACHE
        .get_or_init(key, || async move {
            let cache_path = download_package(&name, &version, &tarball_url).await?;

            let _permit = get_clone_semaphore().acquire().await.ok()?;
            clone_package(&cache_path, &target_path, &name, &version)
                .await
                .inspect_err(|e| {
                    tracing::warn!(
                        "Clone failed: {}@{} to {}: {}",
                        name,
                        version,
                        target_path.display(),
                        e
                    )
                })
                .ok()?;

            STATS.cloned.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Cloned: {}@{} to {}", name, version, target_path.display());
            Some(())
        })
        .await
        .is_some()
}

/// Pipeline receiver that wraps an inner receiver and forwards events to channels.
pub struct PipelineReceiver<R: EventReceiver> {
    download_tx: mpsc::UnboundedSender<OwnedPackageInfo>,
    clone_tx: mpsc::UnboundedSender<CloneMessage>,
    inner: R,
}

/// Channels returned when creating a PipelineReceiver
pub struct PipelineChannels {
    pub download_rx: mpsc::UnboundedReceiver<OwnedPackageInfo>,
    pub clone_rx: mpsc::UnboundedReceiver<CloneMessage>,
}

impl<R: EventReceiver> PipelineReceiver<R> {
    /// Create a new pipeline receiver wrapping an inner receiver
    pub fn new(inner: R) -> (Self, PipelineChannels) {
        let (download_tx, download_rx) = mpsc::unbounded_channel();
        let (clone_tx, clone_rx) = mpsc::unbounded_channel();
        (
            Self {
                download_tx,
                clone_tx,
                inner,
            },
            PipelineChannels {
                download_rx,
                clone_rx,
            },
        )
    }
}

impl<R: EventReceiver> EventReceiver for PipelineReceiver<R> {
    fn on_event(&self, event: BuildEvent<'_>) {
        // Forward to inner receiver first (for progress bar updates)
        self.inner.on_event(event);

        match event {
            BuildEvent::PackageResolved(info)
                if info.tarball_url.is_some() && info.is_platform_compatible() =>
            {
                let _ = self
                    .download_tx
                    .send(OwnedPackageInfo::from_tarball_info(&info));
            }
            BuildEvent::PackagePlaced {
                package,
                path,
                parent_path,
            } if package.tarball_url.is_some() && package.is_platform_compatible() => {
                let _ = self.clone_tx.send(CloneMessage {
                    info: OwnedPackageInfo::from_tarball_info(&package),
                    path: path.to_path_buf(),
                    parent_path: parent_path.map(|p| p.to_path_buf()),
                });
            }
            _ => {}
        }
    }
}

/// Pipeline worker handles for awaiting completion
pub struct PipelineHandles {
    pub download_handle: tokio::task::JoinHandle<()>,
    pub clone_handle: tokio::task::JoinHandle<()>,
}

impl PipelineHandles {
    /// Wait for all pipeline workers to complete
    pub async fn await_completion(self) {
        let _ = self.download_handle.await;
        let _ = self.clone_handle.await;
    }
}

/// Start all pipeline workers and return handles.
fn start_pipeline_workers(channels: PipelineChannels, cwd: PathBuf) -> PipelineHandles {
    let download_handle = tokio::spawn(async move {
        let mut rx = channels.download_rx;
        while let Some(info) = rx.recv().await {
            let Some(tarball_url) = info.tarball_url else {
                continue;
            };
            let name = info.name;
            let version = info.version;
            tokio::spawn(async move {
                download_package(&name, &version, &tarball_url).await;
            });
        }
    });

    let clone_handle = tokio::spawn(async move {
        let mut rx = channels.clone_rx;
        while let Some(msg) = rx.recv().await {
            let Some(tarball_url) = msg.info.tarball_url else {
                continue;
            };
            let name = msg.info.name;
            let version = msg.info.version;
            let target = cwd.join(&msg.path);
            let parent_path = msg.parent_path.map(|p| cwd.join(&p));
            tokio::spawn(async move {
                if let Some(ref parent) = parent_path {
                    CLONE_CACHE
                        .wait_if_pending(&parent.to_string_lossy().to_string())
                        .await;
                }
                clone_package_once(&name, &version, &tarball_url, &target).await;
            });
        }
    });

    PipelineHandles {
        download_handle,
        clone_handle,
    }
}

// ============ Pipeline Resolve API ============

/// Result of pipeline-based dependency resolution.
pub struct PipelineResult {
    pub package_lock: utoo_ruborist::lock::PackageLock,
    pub handles: PipelineHandles,
}

/// Resolve dependencies with pipeline: concurrent download/clone during resolution.
///
/// Creates PipelineReceiver, starts workers, runs build_deps, saves lock file.
/// Returns both the lock and worker handles for the caller to await after install.
pub async fn resolve_with_pipeline(root_path: &std::path::Path) -> anyhow::Result<PipelineResult> {
    use crate::helper::lock::save_package_lock;
    use crate::helper::ruborist_context::Context;
    use crate::util::logger::{ProgressReceiver, finish_progress_bar, start_progress_bar};

    start_progress_bar();

    let (receiver, channels) = PipelineReceiver::new(ProgressReceiver);
    let options = Context::deps_options(root_path.to_path_buf(), receiver).await;
    let handles = start_pipeline_workers(channels, root_path.to_path_buf());

    let package_lock = utoo_ruborist::service::build_deps(options).await?;

    finish_progress_bar("package-lock.json resolved");
    save_package_lock(root_path, &package_lock).await?;

    Ok(PipelineResult {
        package_lock,
        handles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoo_ruborist::progress::NoopReceiver;

    #[test]
    fn test_pipeline_receiver_filters_events() {
        let (receiver, mut channels) = PipelineReceiver::new(NoopReceiver);

        // Should forward PackageResolved with tarball_url
        receiver.on_event(BuildEvent::PackageResolved(PackageTarballInfo {
            name: "react",
            version: "18.2.0",
            tarball_url: Some("https://registry.npmjs.org/react/-/react-18.2.0.tgz"),
            integrity: Some("sha512-xxx"),
            os: None,
            cpu: None,
        }));

        // Should not forward PackageResolved without tarball_url
        receiver.on_event(BuildEvent::PackageResolved(PackageTarballInfo {
            name: "local-pkg",
            version: "1.0.0",
            tarball_url: None,
            integrity: None,
            os: None,
            cpu: None,
        }));

        // Should not forward other events
        receiver.on_event(BuildEvent::PreloadStart { count: 10 });

        // Only one message should be in the download channel
        assert!(channels.download_rx.try_recv().is_ok());
        assert!(channels.download_rx.try_recv().is_err());
    }
}
