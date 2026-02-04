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
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};

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
pub static STATS: Lazy<PipelineStats> = Lazy::new(PipelineStats::new);

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

/// Message types for clone worker
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

            // Phase 1: Network download (semaphore controlled)
            let bytes = {
                let _permit = get_download_semaphore().acquire().await.ok()?;
                match download_bytes(&tarball_url).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("Download failed: {}@{}: {}", name, version, e);
                        return None;
                    }
                }
            };

            // Phase 2: Extract and write
            match extract_and_write(bytes, &cache_path).await {
                Ok(()) => {
                    STATS.downloaded.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("Extracted: {}@{}", name, version);
                    Some(cache_path)
                }
                Err(e) => {
                    tracing::warn!("Extract failed: {}@{}: {}", name, version, e);
                    None
                }
            }
        })
        .await
        .map(|arc| (*arc).clone())
}

/// Validate that the package at target_path has matching name and version
async fn validate_target_package(target_path: &std::path::Path, name: &str, version: &str) -> bool {
    let pkg_json_path = target_path.join("package.json");
    let Ok(content) = crate::fs::read_to_string(&pkg_json_path).await else {
        return false;
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    pkg.get("name").and_then(|v| v.as_str()) == Some(name)
        && pkg.get("version").and_then(|v| v.as_str()) == Some(version)
}

/// Clone a package to target path, using global OnceMap for deduplication.
pub async fn clone_package_once(
    name: &str,
    version: &str,
    tarball_url: &str,
    target_path: &std::path::Path,
) -> bool {
    use crate::util::cloner::clone_package;

    // Fast path: skip if target already exists with matching name/version
    if validate_target_package(target_path, name, version).await {
        tracing::debug!("Skip clone, already valid: {}@{}", name, version);
        return true;
    }

    let key = target_path.to_string_lossy().to_string();
    let name = name.to_string();
    let version = version.to_string();
    let tarball_url = tarball_url.to_string();
    let target_path = target_path.to_path_buf();

    let result = CLONE_CACHE
        .get_or_init(key, || async move {
            // Phase 1: Download (via OnceMap, may already be downloaded)
            let cache_path = match download_package(&name, &version, &tarball_url).await {
                Some(p) => p,
                None => {
                    tracing::warn!("Clone failed: download failed {}@{}", name, version);
                    return None;
                }
            };

            // Phase 2: Clone with semaphore
            let _permit = get_clone_semaphore().acquire().await.ok()?;
            match clone_package(&cache_path, &target_path, &name, &version).await {
                Ok(()) => {
                    STATS.cloned.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("Cloned: {}@{} to {}", name, version, target_path.display());
                    Some(())
                }
                Err(e) => {
                    tracing::warn!(
                        "Clone failed: {}@{} to {}: {}",
                        name,
                        version,
                        target_path.display(),
                        e
                    );
                    None
                }
            }
        })
        .await;

    result.is_some()
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
            // Handle PackageResolved for preload phase downloading
            BuildEvent::PackageResolved(info) => {
                if info.tarball_url.is_some() {
                    // Skip packages that are incompatible with current platform
                    if let Some(os) = info.os
                        && !is_os_compatible(os)
                    {
                        tracing::debug!("Pipeline skip (os): {}@{}", info.name, info.version);
                        return;
                    }
                    if let Some(cpu) = info.cpu
                        && !is_cpu_compatible(cpu)
                    {
                        tracing::debug!("Pipeline skip (cpu): {}@{}", info.name, info.version);
                        return;
                    }
                    let _ = self
                        .download_tx
                        .send(OwnedPackageInfo::from_tarball_info(&info));
                }
            }
            // Handle PackagePlaced for BFS phase cloning
            BuildEvent::PackagePlaced {
                package,
                path,
                parent_path,
                ..
            } => {
                if package.tarball_url.is_some() {
                    // Skip packages that are incompatible with current platform
                    if let Some(os) = package.os
                        && !is_os_compatible(os)
                    {
                        tracing::debug!(
                            "Pipeline clone skip (os): {}@{}",
                            package.name,
                            package.version
                        );
                        return;
                    }
                    if let Some(cpu) = package.cpu
                        && !is_cpu_compatible(cpu)
                    {
                        tracing::debug!(
                            "Pipeline clone skip (cpu): {}@{}",
                            package.name,
                            package.version
                        );
                        return;
                    }
                    let _ = self.clone_tx.send(CloneMessage {
                        info: OwnedPackageInfo::from_tarball_info(&package),
                        path: path.to_path_buf(),
                        parent_path: parent_path.map(|p| p.to_path_buf()),
                    });
                }
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

/// Pipeline installer
pub struct PipelineInstaller;

impl PipelineInstaller {
    pub fn new() -> Self {
        Self
    }

    /// Start all pipeline workers and return handles
    pub fn start_workers(&self, channels: PipelineChannels, cwd: PathBuf) -> PipelineHandles {
        PipelineHandles {
            download_handle: self.start_download_worker(channels.download_rx),
            clone_handle: self.start_clone_worker(channels.clone_rx, cwd),
        }
    }

    /// Start the download worker (fire-and-forget downloads)
    pub fn start_download_worker(
        &self,
        mut rx: mpsc::UnboundedReceiver<OwnedPackageInfo>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(info) = rx.recv().await {
                let Some(tarball_url) = info.tarball_url else {
                    continue;
                };

                let name = info.name;
                let version = info.version;

                // Fire-and-forget: clone phase will wait via OnceMap
                tokio::spawn(async move {
                    download_package(&name, &version, &tarball_url).await;
                });
            }
        })
    }

    /// Start the clone worker (processes packages with pre-registration pattern)
    ///
    /// Uses synchronous pre-registration in CLONE_CACHE to ensure parent packages
    /// are always registered before children, enabling proper dependency ordering
    /// while maintaining maximum concurrency.
    pub fn start_clone_worker(
        &self,
        mut rx: mpsc::UnboundedReceiver<CloneMessage>,
        cwd: PathBuf,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let Some(tarball_url) = msg.info.tarball_url else {
                    continue;
                };

                let target_path = cwd.join(&msg.path);
                let key = target_path.to_string_lossy().to_string();

                // Fast path: skip if target already exists with matching name/version
                if validate_target_package(&target_path, &msg.info.name, &msg.info.version).await {
                    tracing::debug!(
                        "Skip clone, already valid: {}@{}",
                        msg.info.name,
                        msg.info.version
                    );
                    continue;
                }

                // Step 1: Pre-register synchronously to claim this key
                // This ensures parent is always registered before child
                let Some(notify) = CLONE_CACHE.register(key.clone()) else {
                    // Already registered by someone else, skip
                    tracing::debug!("Clone already registered: {}", key);
                    continue;
                };

                // Step 2: Compute parent key if exists
                let parent_key = msg
                    .parent_path
                    .map(|p| cwd.join(p).to_string_lossy().to_string());

                let name = msg.info.name;
                let version = msg.info.version;

                // Step 3: Spawn task to do async work
                tokio::spawn(async move {
                    // Wait for parent to complete (parent is guaranteed to be registered)
                    if let Some(ref parent_key) = parent_key {
                        CLONE_CACHE.wait_if_pending(parent_key).await;
                    }

                    // Phase 1: Download (via OnceMap, may already be downloaded)
                    let cache_path = match download_package(&name, &version, &tarball_url).await {
                        Some(p) => p,
                        None => {
                            tracing::warn!("Clone failed: download failed {}@{}", name, version);
                            CLONE_CACHE.complete(key, None, notify);
                            return;
                        }
                    };

                    // Phase 2: Clone with semaphore
                    use crate::util::cloner::clone_package;
                    let result = {
                        let _permit = get_clone_semaphore().acquire().await.ok();
                        if _permit.is_none() {
                            CLONE_CACHE.complete(key, None, notify);
                            return;
                        }
                        clone_package(&cache_path, &target_path, &name, &version).await
                    };

                    match result {
                        Ok(()) => {
                            STATS.cloned.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!(
                                "Cloned: {}@{} to {}",
                                name,
                                version,
                                target_path.display()
                            );
                            CLONE_CACHE.complete(key, Some(()), notify);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Clone failed: {}@{} to {}: {}",
                                name,
                                version,
                                target_path.display(),
                                e
                            );
                            CLONE_CACHE.complete(key, None, notify);
                        }
                    }
                });
            }
        })
    }
}

impl Default for PipelineInstaller {
    fn default() -> Self {
        Self::new()
    }
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
