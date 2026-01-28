//! Pipeline installer for concurrent manifest resolution and tarball downloading.
//!
//! This module implements a pipeline architecture similar to bun's approach:
//! - Manifest fetching and tarball downloading happen concurrently
//! - When a package is resolved, its tarball download starts immediately
//! - Uses global OnceMap to deduplicate requests and share results across phases

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use once_cell::sync::Lazy;
use tokio::sync::{Semaphore, mpsc};
use utoo_ruborist::progress::{BuildEvent, EventReceiver, PackagePlacedInfo, PackageResolvedInfo};

use crate::util::cache::get_cache_dir;
use crate::util::config::get_manifests_concurrency_limit_sync;
use crate::util::downloader::{download_bytes, extract_and_write};
use crate::util::oncemap::OnceMap;
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};

// ============ Pipeline Statistics ============

/// Statistics for pipeline stages with timing information
pub struct PipelineStats {
    /// Number of packages currently downloading
    pub downloading: AtomicUsize,
    /// Number of packages currently extracting (untgz)
    pub extracting: AtomicUsize,
    /// Total downloads completed
    pub downloaded: AtomicUsize,
    /// Total extractions completed
    pub extracted: AtomicUsize,

    // Timing stats (in microseconds for precision)
    /// Total time spent on network downloads (µs)
    pub network_time_us: AtomicU64,
    /// Total time spent on decompression (µs)
    pub decompress_time_us: AtomicU64,
    /// Total time spent on file writes (µs)
    pub write_time_us: AtomicU64,
    /// Total bytes downloaded
    pub bytes_downloaded: AtomicU64,
    /// Total bytes written
    pub bytes_written: AtomicU64,
}

impl PipelineStats {
    pub const fn new() -> Self {
        Self {
            downloading: AtomicUsize::new(0),
            extracting: AtomicUsize::new(0),
            downloaded: AtomicUsize::new(0),
            extracted: AtomicUsize::new(0),
            network_time_us: AtomicU64::new(0),
            decompress_time_us: AtomicU64::new(0),
            write_time_us: AtomicU64::new(0),
            bytes_downloaded: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
        }
    }

    pub fn downloading(&self) -> usize {
        self.downloading.load(Ordering::Relaxed)
    }

    pub fn extracting(&self) -> usize {
        self.extracting.load(Ordering::Relaxed)
    }

    pub fn downloaded(&self) -> usize {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn extracted(&self) -> usize {
        self.extracted.load(Ordering::Relaxed)
    }

    pub fn add_network_time(&self, us: u64) {
        self.network_time_us.fetch_add(us, Ordering::Relaxed);
    }

    pub fn add_decompress_time(&self, us: u64) {
        self.decompress_time_us.fetch_add(us, Ordering::Relaxed);
    }

    pub fn add_write_time(&self, us: u64) {
        self.write_time_us.fetch_add(us, Ordering::Relaxed);
    }

    pub fn add_bytes_downloaded(&self, bytes: u64) {
        self.bytes_downloaded.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_bytes_written(&self, bytes: u64) {
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Print timing summary
    pub fn print_summary(&self) {
        let network_ms = self.network_time_us.load(Ordering::Relaxed) / 1000;
        let decompress_ms = self.decompress_time_us.load(Ordering::Relaxed) / 1000;
        let write_ms = self.write_time_us.load(Ordering::Relaxed) / 1000;
        let downloaded_mb = self.bytes_downloaded.load(Ordering::Relaxed) as f64 / 1024.0 / 1024.0;
        let written_mb = self.bytes_written.load(Ordering::Relaxed) as f64 / 1024.0 / 1024.0;
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let max_blocking = (num_cpus * 2).max(12); // same as main.rs config

        tracing::info!(
            "Pipeline stats: network={}ms, decompress={}ms, write={}ms, downloaded={:.1}MB, written={:.1}MB, cpus={}, max_blocking={}",
            network_ms,
            decompress_ms,
            write_ms,
            downloaded_mb,
            written_mb,
            num_cpus,
            max_blocking
        );
    }
}

/// Global pipeline statistics
pub static STATS: Lazy<PipelineStats> = Lazy::new(PipelineStats::new);

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path
/// This ensures:
/// 1. Pipeline downloads are deduplicated
/// 2. Install phase can wait for and reuse ongoing pipeline downloads
static DOWNLOAD_CACHE: Lazy<OnceMap<String, PathBuf>> = Lazy::new(OnceMap::new);

/// Global clone cache shared between pipeline and install phases.
/// Key: target path (e.g., "node_modules/lodash"), Value: ()
/// This ensures:
/// 1. Pipeline clones are deduplicated
/// 2. Install phase can wait for and skip ongoing pipeline clones
static CLONE_CACHE: Lazy<OnceMap<String, ()>> = Lazy::new(OnceMap::new);

/// Global semaphore for download concurrency control (initialized at runtime)
static DOWNLOAD_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Global semaphore for clone concurrency control (initialized at runtime)
static CLONE_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_clone_semaphore() -> &'static Arc<Semaphore> {
    CLONE_SEMAPHORE.get_or_init(|| {
        let limit = get_manifests_concurrency_limit_sync();
        tracing::debug!("Initializing clone semaphore with limit: {}", limit);
        Arc::new(Semaphore::new(limit))
    })
}

/// Message types for clone worker
#[derive(Debug)]
pub enum CloneMessage {
    /// A package to clone
    Package(PackagePlacedInfo),
    /// Signal that current level is complete
    LevelComplete,
}

fn get_download_semaphore() -> &'static Arc<Semaphore> {
    DOWNLOAD_SEMAPHORE.get_or_init(|| {
        let limit = get_manifests_concurrency_limit_sync();
        tracing::debug!("Initializing download semaphore with limit: {}", limit);
        Arc::new(Semaphore::new(limit))
    })
}

/// Download a package tarball, using global OnceMap for deduplication.
/// Can be called from both pipeline and install phases.
///
/// Architecture: semaphore only controls network download, not extraction.
/// This allows more parallelism: 64 downloads + unlimited extractions.
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
            }; // permit released here

            // Phase 2: Extract and write (no semaphore, uses blocking threads)
            match extract_and_write(bytes, &cache_path).await {
                Ok(()) => {
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

/// Clone a package to target path, using global OnceMap for deduplication.
/// Returns true if clone was successful (or already done), false if failed.
///
/// Note: Caller is responsible for ensuring correct clone order (parent before child).
/// The clone worker handles this by processing levels sequentially.
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

/// Pipeline receiver that wraps an inner receiver and forwards events to both
/// the inner receiver (for UI progress) and channels (for downloads and clones).
pub struct PipelineReceiver<R: EventReceiver> {
    download_tx: mpsc::UnboundedSender<PackageResolvedInfo>,
    clone_tx: mpsc::UnboundedSender<CloneMessage>,
    inner: R,
}

/// Channels returned when creating a PipelineReceiver
pub struct PipelineChannels {
    pub download_rx: mpsc::UnboundedReceiver<PackageResolvedInfo>,
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
    fn on_event(&self, event: BuildEvent) {
        // Forward to inner receiver first (for progress bar updates)
        self.inner.on_event(event.clone());

        match event {
            // Handle PackageResolved for preload phase downloading
            BuildEvent::PackageResolved(info) => {
                if info.tarball_url.is_some() {
                    // Skip packages that are incompatible with current platform
                    if let Some(ref os) = info.os
                        && !is_os_compatible(os)
                    {
                        tracing::debug!("Pipeline skip (os): {}@{}", info.name, info.version);
                        return;
                    }
                    if let Some(ref cpu) = info.cpu
                        && !is_cpu_compatible(cpu)
                    {
                        tracing::debug!("Pipeline skip (cpu): {}@{}", info.name, info.version);
                        return;
                    }
                    let _ = self.download_tx.send(info);
                }
            }
            // Handle PackagePlaced for BFS phase cloning
            BuildEvent::PackagePlaced(info) => {
                if info.tarball_url.is_some() {
                    // Skip packages that are incompatible with current platform
                    if let Some(ref os) = info.os
                        && !is_os_compatible(os)
                    {
                        tracing::debug!("Pipeline clone skip (os): {}@{}", info.name, info.version);
                        return;
                    }
                    if let Some(ref cpu) = info.cpu
                        && !is_cpu_compatible(cpu)
                    {
                        tracing::debug!(
                            "Pipeline clone skip (cpu): {}@{}",
                            info.name,
                            info.version
                        );
                        return;
                    }
                    let _ = self.clone_tx.send(CloneMessage::Package(info));
                }
            }
            // Handle LevelComplete to signal clone worker to process current level
            BuildEvent::LevelComplete { .. } => {
                let _ = self.clone_tx.send(CloneMessage::LevelComplete);
            }
            _ => {}
        }
    }
}

/// Pipeline installer that runs manifest resolution, tarball downloading, and cloning concurrently
pub struct PipelineInstaller;

impl PipelineInstaller {
    /// Create a new pipeline installer
    pub fn new() -> Self {
        Self
    }

    /// Start the download worker that processes resolved packages (preload phase)
    ///
    /// Fire-and-forget: spawns download tasks without waiting.
    /// Clone phase will wait for needed packages via OnceMap.
    pub fn start_download_worker(
        &self,
        mut rx: mpsc::UnboundedReceiver<PackageResolvedInfo>,
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

    /// Start the clone worker that processes placed packages during BFS phase.
    ///
    /// Processes packages level by level:
    /// 1. Collect all packages for current level
    /// 2. On LevelComplete, clone all collected packages concurrently
    /// 3. Wait for all clones to complete before processing next level
    pub fn start_clone_worker(
        &self,
        mut rx: mpsc::UnboundedReceiver<CloneMessage>,
        cwd: std::path::PathBuf,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut current_level: usize = 0;
            let mut pending: Vec<PackagePlacedInfo> = Vec::new();

            while let Some(msg) = rx.recv().await {
                match msg {
                    CloneMessage::Package(info) => {
                        // Collect packages for current level
                        pending.push(info);
                    }
                    CloneMessage::LevelComplete => {
                        // Clone all packages in current level concurrently
                        let tasks: Vec<_> = pending
                            .drain(..)
                            .filter_map(|info| {
                                let tarball_url = info.tarball_url?;
                                let name = info.name;
                                let version = info.version;
                                let target_path = cwd.join(&info.path);

                                Some(tokio::spawn(async move {
                                    clone_package_once(&name, &version, &tarball_url, &target_path)
                                        .await
                                }))
                            })
                            .collect();

                        // Wait for all clones in this level to complete
                        for task in tasks {
                            let _ = task.await;
                        }

                        tracing::debug!("Clone level {} complete", current_level);
                        current_level += 1;
                    }
                }
            }

            // Handle any remaining packages (shouldn't happen normally)
            if !pending.is_empty() {
                tracing::warn!("Clone worker ended with {} pending packages", pending.len());
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
        receiver.on_event(BuildEvent::PackageResolved(PackageResolvedInfo {
            name: "react".to_string(),
            version: "18.2.0".to_string(),
            tarball_url: Some("https://registry.npmjs.org/react/-/react-18.2.0.tgz".to_string()),
            integrity: Some("sha512-xxx".to_string()),
            os: None,
            cpu: None,
        }));

        // Should not forward PackageResolved without tarball_url
        receiver.on_event(BuildEvent::PackageResolved(PackageResolvedInfo {
            name: "local-pkg".to_string(),
            version: "1.0.0".to_string(),
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
