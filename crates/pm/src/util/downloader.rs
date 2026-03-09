use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use tokio::sync::Semaphore;
use tokio_retry::RetryIf;
use utoo_ruborist::spec::Protocol;

use super::cache::get_cache_dir;
use super::extractor::extract_and_write;
use super::oncemap::OnceMap;
use super::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};
use super::user_config::get_manifests_concurrency_limit_sync;

// Global downloader client - no pool limit, concurrency controlled by OnceMap
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path.
static DOWNLOAD_CACHE: Lazy<OnceMap<String, PathBuf>> = Lazy::new(OnceMap::new);

/// Semaphore controlling concurrent download count.
static DOWNLOAD_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

/// Number of fresh downloads (not cache hits).
static DOWNLOAD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of fresh downloads performed.
pub fn download_count() -> usize {
    DOWNLOAD_COUNT.load(Ordering::Relaxed)
}

/// Check whether a tarball URL refers to a git-resolved package.
pub fn is_git_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::Git))
}

/// Look up the cache path for a git-resolved package.
///
/// Git packages are cloned during BFS resolution (inside ruborist) and
/// stored at `<cache_dir>/<name>/<commit_sha>/`.
pub async fn git_cache_lookup(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    let commit_sha = tarball_url.split_once('#').map(|(_, frag)| frag)?;
    if commit_sha.contains("..") || commit_sha.contains('/') || commit_sha.contains('\\') {
        tracing::warn!("Suspicious commit SHA fragment in URL: {}", tarball_url);
        return None;
    }
    let cache_dir = get_cache_dir();
    let cache_path = cache_dir.join(name).join(commit_sha);
    if crate::fs::try_exists(&cache_path.join("_resolved"))
        .await
        .unwrap_or(false)
    {
        tracing::debug!("Git package cache hit: {}@{}", name, version);
        return Some(cache_path);
    }
    tracing::warn!(
        "Git package {}@{} not found in cache, expected pre-resolution",
        name,
        version
    );
    None
}

/// Resolve the local cache path for a package, downloading if necessary.
///
/// Routes git URLs to [`git_cache_lookup`] and registry tarballs to
/// [`download_to_cache`].
pub async fn resolve_cache_path(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    if is_git_url(tarball_url) {
        git_cache_lookup(name, version, tarball_url).await
    } else {
        download_to_cache(name, version, tarball_url).await
    }
}

/// Download a registry tarball to the global cache directory, returning the cache path.
///
/// Uses `OnceMap` to deduplicate: the same `name@version` is only downloaded once,
/// even when called concurrently from multiple tasks (pipeline workers, install phase, etc.).
///
/// For git-resolved packages, use [`resolve_cache_path`] instead.
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
            let semaphore = DOWNLOAD_SEMAPHORE
                .get_or_init(|| Semaphore::new(get_manifests_concurrency_limit_sync()));
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

            let response = match DOWNLOADER_CLIENT.get(url).send().await {
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
                            "Retry {}/10 - Stream error: {}, url: {}",
                            attempt + 1,
                            e,
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
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;
    use tempfile::TempDir;

    use super::*;

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
