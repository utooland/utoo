use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use tokio::sync::Semaphore;
use tokio_retry::RetryIf;
use utoo_ruborist::file::file_cache_slot;
use utoo_ruborist::http::http_cache_slot;
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

/// Check whether a tarball URL refers to a `file:` dependency.
///
/// Pinned URL shape is `file:<absolute_path>` (see ruborist's
/// `finalize_non_registry_manifest`).
pub fn is_file_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::File))
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

/// Look up a ruborist-seeded cache slot at `<cache_dir>/<name>/<slot>/`.
///
/// Returns `Some(path)` only if the slot's `_resolved` marker exists —
/// otherwise returns `None` so the caller can fall through to the next
/// routing step (typically the registry download path).
async fn slot_cache_lookup(name: &str, slot: String) -> Option<PathBuf> {
    let cache_path = get_cache_dir().join(name).join(slot);
    if crate::fs::try_exists(&cache_path.join("_resolved"))
        .await
        .unwrap_or(false)
    {
        Some(cache_path)
    } else {
        None
    }
}

/// Look up the cache path for a `file:` dependency.
pub async fn file_cache_lookup(name: &str, tarball_url: &str) -> Option<PathBuf> {
    let abs_path = tarball_url.strip_prefix("file:")?;
    let hit = slot_cache_lookup(name, file_cache_slot(std::path::Path::new(abs_path))).await;
    if hit.is_some() {
        tracing::debug!("file: dep cache hit: {} ({})", name, tarball_url);
    }
    hit
}

/// Look up the cache path for an HTTP(S) tarball dep.
pub async fn http_tarball_cache_lookup(name: &str, tarball_url: &str) -> Option<PathBuf> {
    let hit = slot_cache_lookup(name, http_cache_slot(tarball_url)).await;
    if hit.is_some() {
        tracing::debug!("HTTP tarball cache hit: {} ({})", name, tarball_url);
    }
    hit
}

/// Resolve the local cache path for a package, downloading if necessary.
///
/// Routing order:
/// 1. Git URLs → [`git_cache_lookup`] (cache keyed on commit sha)
/// 2. `file:` URLs → [`file_cache_lookup`] (keyed on absolute-path hash);
///    the extracted/copied tree was seeded by ruborist BFS.
/// 3. Other non-git URLs: try [`http_tarball_cache_lookup`] (keyed on URL
///    hash); if present, the tarball was pre-extracted by BFS.
/// 4. Fall through to [`download_to_cache`] for registry tarball URLs
///    (keyed on `<name>/<version>`).
pub async fn resolve_cache_path(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    if is_git_url(tarball_url) {
        return git_cache_lookup(name, version, tarball_url).await;
    }
    if is_file_url(tarball_url) {
        return file_cache_lookup(name, tarball_url).await;
    }
    if let Some(p) = http_tarball_cache_lookup(name, tarball_url).await {
        return Some(p);
    }
    download_to_cache(name, version, tarball_url).await
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
