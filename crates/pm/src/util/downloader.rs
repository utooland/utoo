use anyhow::{Context, Result};
use std::error::Error as _;
use bytes::Bytes;
use reqwest::StatusCode;
use std::path::PathBuf;
use std::sync::{LazyLock, OnceLock};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use tokio_retry::RetryIf;

use super::cache::get_cache_dir;
use super::extractor::extract_and_write;
use super::http::client_builder;
use super::oncemap::OnceMap;
use super::retry::{RetryableError, create_retry_strategy};

/// HTTP/1.1 client for tarball downloads.
///
/// Uses HTTP/1.1 (not HTTP/2) because bulk tarball downloads benefit from
/// TCP-level parallelism — multiple HTTP/1.1 connections get higher aggregate
/// bandwidth from rate-limited CDNs than a single multiplexed HTTP/2 connection.
/// Concurrency is controlled by [`DOWNLOAD_SEMAPHORE`].
static DOWNLOAD_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    client_builder()
        .http1_only()
        .connect_timeout(std::time::Duration::from_secs(5))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build download client")
});

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path.
static DOWNLOAD_CACHE: LazyLock<OnceMap<String, PathBuf>> = LazyLock::new(OnceMap::new);

/// Semaphore controlling concurrent download count.
static DOWNLOAD_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

/// Number of fresh downloads (not cache hits).
static DOWNLOAD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of fresh downloads performed.
pub fn download_count() -> usize {
    DOWNLOAD_COUNT.load(Ordering::Relaxed)
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

            // Download (semaphore controlled)
            // Uses a lower limit than manifest concurrency because each HTTP/1.1
            // download opens a separate TCP connection (unlike HTTP/2 manifests).
            let semaphore = DOWNLOAD_SEMAPHORE
                .get_or_init(|| Semaphore::new(48));
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
                        tracing::warn!(
                            "Retry {}/10 - Stream error: {}, source: {:?}, url: {}",
                            attempt + 1,
                            e,
                            e.source(),
                            url
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
}
