//! Pipeline installer for concurrent manifest resolution and tarball downloading.
//!
//! This module implements a pipeline architecture similar to bun's approach:
//! - Manifest fetching and tarball downloading happen concurrently
//! - When a package is resolved, its tarball download starts immediately
//! - Uses global OnceMap to deduplicate requests and share results across phases

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use once_cell::sync::Lazy;
use tokio::sync::{mpsc, Semaphore};
use utoo_ruborist::progress::{BuildEvent, EventReceiver, PackageResolvedInfo};

use crate::util::cache::get_cache_dir;
use crate::util::downloader::download;
use crate::util::oncemap::OnceMap;
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};

/// Default concurrent limit for tarball downloads
const DEFAULT_CONCURRENCY: usize = 64;

// ============ Pipeline Statistics ============

/// Statistics for pipeline stages
pub struct PipelineStats {
    /// Number of packages currently downloading
    pub downloading: AtomicUsize,
    /// Number of packages currently extracting (untgz)
    pub extracting: AtomicUsize,
    /// Total downloads completed
    pub downloaded: AtomicUsize,
    /// Total extractions completed
    pub extracted: AtomicUsize,
}

impl PipelineStats {
    pub const fn new() -> Self {
        Self {
            downloading: AtomicUsize::new(0),
            extracting: AtomicUsize::new(0),
            downloaded: AtomicUsize::new(0),
            extracted: AtomicUsize::new(0),
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
}

/// Global pipeline statistics
pub static STATS: Lazy<PipelineStats> = Lazy::new(PipelineStats::new);

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path
/// This ensures:
/// 1. Pipeline downloads are deduplicated
/// 2. Install phase can wait for and reuse ongoing pipeline downloads
static DOWNLOAD_CACHE: Lazy<OnceMap<String, PathBuf>> = Lazy::new(OnceMap::new);

/// Global semaphore for download concurrency control
static DOWNLOAD_SEMAPHORE: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(DEFAULT_CONCURRENCY)));

/// Download a package tarball, using global OnceMap for deduplication.
/// Can be called from both pipeline and install phases.
pub async fn download_package(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    let key = format!("{}@{}", name, version);
    let cache_dir = get_cache_dir();
    let name = name.to_string();
    let version = version.to_string();
    let tarball_url = tarball_url.to_string();

    DOWNLOAD_CACHE
        .get_or_init(key, || async move {
            let _permit = DOWNLOAD_SEMAPHORE.acquire().await.ok()?;
            let cache_path = cache_dir.join(&name).join(&version);

            // Stats are tracked inside download() for accurate timing
            match download(&tarball_url, &cache_path).await {
                Ok(()) => {
                    tracing::debug!("Downloaded: {}@{}", name, version);
                    Some(cache_path)
                }
                Err(e) => {
                    tracing::warn!("Download failed: {}@{}: {}", name, version, e);
                    None
                }
            }
        })
        .await
        .map(|arc| (*arc).clone())
}

/// Pipeline receiver that wraps an inner receiver and forwards events to both
/// the inner receiver (for UI progress) and a channel (for tarball downloads).
pub struct PipelineReceiver<R: EventReceiver> {
    tx: mpsc::UnboundedSender<PackageResolvedInfo>,
    inner: R,
}

impl<R: EventReceiver> PipelineReceiver<R> {
    /// Create a new pipeline receiver wrapping an inner receiver
    pub fn new(inner: R) -> (Self, mpsc::UnboundedReceiver<PackageResolvedInfo>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx, inner }, rx)
    }
}

impl<R: EventReceiver> EventReceiver for PipelineReceiver<R> {
    fn on_event(&self, event: BuildEvent) {
        // Forward to inner receiver first (for progress bar updates)
        self.inner.on_event(event.clone());

        // Then handle PackageResolved for pipeline downloading
        if let BuildEvent::PackageResolved(info) = event {
            // Only send if we have a tarball URL and platform is compatible
            if info.tarball_url.is_some() {
                // Skip packages that are incompatible with current platform
                if let Some(ref os) = info.os {
                    if !is_os_compatible(os) {
                        tracing::debug!("Pipeline skip (os): {}@{}", info.name, info.version);
                        return;
                    }
                }
                if let Some(ref cpu) = info.cpu {
                    if !is_cpu_compatible(cpu) {
                        tracing::debug!("Pipeline skip (cpu): {}@{}", info.name, info.version);
                        return;
                    }
                }
                let _ = self.tx.send(info);
            }
        }
    }
}

/// Pipeline installer that runs manifest resolution and tarball downloading concurrently
pub struct PipelineInstaller;

impl PipelineInstaller {
    /// Create a new pipeline installer
    pub fn new() -> Self {
        Self
    }

    /// Start the download worker that processes resolved packages
    ///
    /// Fire-and-forget: spawns download tasks without waiting.
    /// Install phase will wait for needed packages via OnceMap.
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

                // Fire-and-forget: install phase will wait via OnceMap
                tokio::spawn(async move {
                    download_package(&name, &version, &tarball_url).await;
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
        let (receiver, mut rx) = PipelineReceiver::new(NoopReceiver);

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

        // Only one message should be in the channel
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }
}
