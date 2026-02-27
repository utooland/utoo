use anyhow::{Context, Result};
use bytes::Bytes;
use reqwest::StatusCode;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_retry::RetryIf;

use super::cache::get_cache_dir;
use super::extractor::extract_and_write;
use super::http::client_builder;
use super::oncemap::OnceMap;
use super::retry::{RetryableError, create_retry_strategy};
use super::user_config::get_manifests_concurrency_limit;

/// Minimum concurrent downloads after adaptive degradation.
const MIN_CONCURRENT_DOWNLOADS: usize = 4;

/// HTTP/1.1 client for tarball downloads.
///
/// Uses HTTP/1.1 (not HTTP/2) because bulk tarball downloads benefit from
/// TCP-level parallelism -- multiple HTTP/1.1 connections get higher aggregate
/// bandwidth from rate-limited CDNs than a single multiplexed HTTP/2 connection.
/// Concurrency is controlled by [`DOWNLOAD_SEMAPHORE`].
static DOWNLOAD_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    client_builder()
        .http1_only()
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build download client")
});

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path.
static DOWNLOAD_CACHE: LazyLock<OnceMap<String, PathBuf>> = LazyLock::new(OnceMap::new);

/// Semaphore controlling concurrent download count.
/// Initialized from `--manifests-concurrency-limit` config (default 64); on
/// network errors, permits are permanently forgotten to shrink the effective
/// pool (adaptive degradation).
static DOWNLOAD_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

/// Current effective concurrency limit (for logging).
/// Initialized from config on first download; 0 means not yet initialized.
static EFFECTIVE_CONCURRENCY: AtomicUsize = AtomicUsize::new(0);

/// Number of fresh downloads (not cache hits).
static DOWNLOAD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of fresh downloads performed.
pub fn download_count() -> usize {
    DOWNLOAD_COUNT.load(Ordering::Relaxed)
}

/// Reduce effective download concurrency by half (floor: [`MIN_CONCURRENT_DOWNLOADS`]).
///
/// Works by permanently forgetting semaphore permits so they are never returned
/// to the pool. This is the same strategy used by Bun's package manager.
fn degrade_concurrency() {
    let current = EFFECTIVE_CONCURRENCY.load(Ordering::Relaxed);
    if current <= MIN_CONCURRENT_DOWNLOADS {
        return;
    }

    // How many permits to remove: shrink to current/2, but keep at least MIN
    let new_limit = (current / 2).max(MIN_CONCURRENT_DOWNLOADS);
    let to_remove = current - new_limit;

    if EFFECTIVE_CONCURRENCY
        .compare_exchange(current, new_limit, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        // Spawn a task to acquire and forget permits, shrinking the pool
        let semaphore = DOWNLOAD_SEMAPHORE
            .get()
            .expect("semaphore must be initialized before degradation");
        tokio::spawn(async move {
            for _ in 0..to_remove {
                if let Ok(permit) = semaphore.acquire().await {
                    permit.forget(); // permanently removes from pool
                }
            }
        });

        tracing::warn!(
            "Download concurrency degraded: {} -> {} (network errors detected)",
            current,
            new_limit
        );
    }
}

/// Download a package tarball to the global cache directory, returning the cache path.
///
/// Uses `OnceMap` to deduplicate: the same `name@version` is only downloaded once,
/// even when called concurrently from multiple tasks (pipeline workers, install phase, etc.).
pub async fn download_to_cache(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
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

            let limit = get_manifests_concurrency_limit().await;
            let semaphore = DOWNLOAD_SEMAPHORE.get_or_init(|| {
                EFFECTIVE_CONCURRENCY.store(limit, Ordering::Relaxed);
                Semaphore::new(limit)
            });
            let _permit = semaphore.acquire().await.ok()?;
            let bytes = download_bytes(&tarball_url)
                .await
                .inspect_err(|e| tracing::warn!("Download failed: {}@{}: {}", name, version, e))
                .ok()?;

            // Extract
            extract_and_write(bytes, &cache_path)
                .await
                .inspect_err(|e| tracing::warn!("Extract failed: {}@{}: {}", name, version, e))
                .ok()?;

            DOWNLOAD_COUNT.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Downloaded: {}@{}", name, version);
            Some(cache_path)
        })
        .await
        .as_deref()
        .cloned()
}

/// Download tarball bytes with retries (network phase only).
pub async fn download_bytes(url: &str) -> Result<Bytes> {
    let retry_count = AtomicU32::new(0);
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let attempt = retry_count.fetch_add(1, Ordering::Relaxed);

            let response = match DOWNLOAD_CLIENT.get(url).send().await {
                Ok(r) => r,
                Err(e) => {
                    degrade_concurrency();
                    tracing::warn!(
                        "Retry {}/10 - Network error: {}, url: {}",
                        attempt + 1,
                        e,
                        url
                    );
                    return Err(RetryableError::Temporary(format!("Network error: {e}")));
                }
            };

            match response.status() {
                StatusCode::OK => {
                    let bytes = response.bytes().await.map_err(|e| {
                        degrade_concurrency();
                        tracing::warn!(
                            "Retry {}/10 - Stream error: {e:#}, url: {url}",
                            attempt + 1,
                        );
                        RetryableError::Temporary(format!("Stream error: {e}"))
                    })?;
                    if attempt > 0 {
                        tracing::info!("Retry succeeded on attempt {}, url: {}", attempt + 1, url);
                    }
                    Ok(bytes)
                }
                StatusCode::NOT_FOUND => {
                    tracing::debug!("URL not found {url}");
                    Err(RetryableError::Permanent(format!("URL not found {url}")))
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    degrade_concurrency();
                    tracing::warn!("Retry {}/10 - HTTP 429, url: {}", attempt + 1, url);
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: 429, url: {url}"
                    )))
                }
                status if status.is_server_error() => {
                    degrade_concurrency();
                    tracing::warn!("Retry {}/10 - HTTP {}, url: {}", attempt + 1, status, url);
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {status}, url: {url}"
                    )))
                }
                status => {
                    tracing::warn!("Retry {}/10 - HTTP {}, url: {}", attempt + 1, status, url);
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {status}, url: {url}"
                    )))
                }
            }
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await
    .context("Download failed after retries")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use tar::Builder;
    use tempfile::TempDir;

    // Helper to create a simple tar.gz archive in memory
    fn create_tar_gz() -> Vec<u8> {
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);
            let mut header = tar::Header::new_gnu();
            let content = b"hello world";
            header.set_path("file.txt").unwrap();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append(&header, &content[..]).unwrap();
            tar.finish().unwrap();
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_data).unwrap();
        encoder.finish().unwrap()
    }

    #[tokio::test]
    async fn test_extract_and_write() {
        let tar_gz = create_tar_gz();
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");

        extract_and_write(Bytes::from(tar_gz), &dest).await.unwrap();

        // _resolved file should exist
        assert!(dest.join("_resolved").exists());
        // Extracted file should exist
        assert!(dest.join("file.txt").exists());
        let content = crate::fs::read_to_string(dest.join("file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_extract_and_write_idempotent() {
        let tar_gz = create_tar_gz();
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");

        // First extraction
        extract_and_write(Bytes::from(tar_gz.clone()), &dest)
            .await
            .unwrap();

        // Second extraction should skip (already resolved)
        extract_and_write(Bytes::from(tar_gz), &dest).await.unwrap();

        assert!(dest.join("_resolved").exists());
        assert!(dest.join("file.txt").exists());
    }

    #[tokio::test]
    async fn test_degrade_concurrency() {
        // Initialize semaphore and effective concurrency
        let _ = DOWNLOAD_SEMAPHORE.get_or_init(|| Semaphore::new(64));
        EFFECTIVE_CONCURRENCY.store(64, Ordering::Relaxed);

        // Degrade: 64 -> 32
        degrade_concurrency();
        // Allow spawned permit-forgetting task to run
        tokio::task::yield_now().await;
        assert_eq!(EFFECTIVE_CONCURRENCY.load(Ordering::Relaxed), 32);

        // Degrade: 32 -> 16
        degrade_concurrency();
        tokio::task::yield_now().await;
        assert_eq!(EFFECTIVE_CONCURRENCY.load(Ordering::Relaxed), 16);

        // Degrade: 16 -> 8
        degrade_concurrency();
        tokio::task::yield_now().await;
        assert_eq!(EFFECTIVE_CONCURRENCY.load(Ordering::Relaxed), 8);

        // Degrade: 8 -> 4
        degrade_concurrency();
        tokio::task::yield_now().await;
        assert_eq!(EFFECTIVE_CONCURRENCY.load(Ordering::Relaxed), 4);

        // Floor: 4 -> 4 (no further degradation)
        degrade_concurrency();
        tokio::task::yield_now().await;
        assert_eq!(EFFECTIVE_CONCURRENCY.load(Ordering::Relaxed), 4);

        // Restore
        EFFECTIVE_CONCURRENCY.store(0, Ordering::Relaxed);
    }
}
