use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use tokio_retry::RetryIf;
use utoo_ruborist::http::{file_cache_slot, http_cache_slot};
use utoo_ruborist::spec::Protocol;

use super::cache::get_cache_dir;
use super::extractor::{extract_and_write, extract_and_write_sync};
use super::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};

// Global downloader client. Concurrency and duplicate work are controlled by
// the caller's scheduler.
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

static DOWNLOAD_COUNT: AtomicUsize = AtomicUsize::new(0);
static REUSE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Process-global counters for tarball outcomes, matching pnpm's
/// vocabulary. Scheduler-level dedupe keeps each unique `(name, version)` pair
/// in exactly one bucket; git/file/link packages bypass this path and are not
/// counted in either bucket.
#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadStats {
    /// Tarballs fetched from the registry this run.
    pub downloaded: usize,
    /// Tarballs served from the local cache (no network).
    pub reused: usize,
}

impl std::ops::Sub for DownloadStats {
    type Output = DownloadStats;
    fn sub(self, rhs: Self) -> Self {
        DownloadStats {
            downloaded: self.downloaded.saturating_sub(rhs.downloaded),
            reused: self.reused.saturating_sub(rhs.reused),
        }
    }
}

/// Snapshot the current download/reuse counters.
pub fn download_stats() -> DownloadStats {
    DownloadStats {
        downloaded: DOWNLOAD_COUNT.load(Ordering::Relaxed),
        reused: REUSE_COUNT.load(Ordering::Relaxed),
    }
}

/// Check whether a tarball URL refers to a git-resolved package.
pub fn is_git_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::Git))
}

/// Check whether a tarball URL should be fetched by the registry downloader.
pub fn is_registry_tarball_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::Http))
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

/// Look up the cache path for a `file:<absolute_tarball>` dependency.
///
/// The URL must already be absolute; call sites that read relative URLs
/// from the lockfile are responsible for re-absolutizing against the
/// project root before reaching the cloner.
pub async fn file_cache_lookup(name: &str, tarball_url: &str) -> Option<PathBuf> {
    let abs_path = tarball_url.strip_prefix("file:")?;
    slot_cache_lookup(name, file_cache_slot(std::path::Path::new(abs_path))).await
}

/// Look up the cache path for an HTTP(S) tarball dep.
pub async fn http_tarball_cache_lookup(name: &str, tarball_url: &str) -> Option<PathBuf> {
    slot_cache_lookup(name, http_cache_slot(tarball_url)).await
}

/// Resolve cache slots that may have been seeded during dependency resolution
/// without falling through to registry download. `Ok(None)` means this is a
/// registry-style HTTP tarball that should be downloaded into `<name>/<version>`.
pub async fn resolve_seeded_cache_path(
    name: &str,
    version: &str,
    tarball_url: &str,
) -> Result<Option<PathBuf>> {
    match tarball_url.parse::<Protocol>() {
        Ok(Protocol::Git) => git_cache_lookup(name, version, tarball_url)
            .await
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("git cache not found for {name}@{version}")),
        Ok(Protocol::File) => file_cache_lookup(name, tarball_url)
            .await
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("file tarball cache not found for {name}@{version}")),
        _ => Ok(http_tarball_cache_lookup(name, tarball_url).await),
    }
}

/// Download and extract a registry tarball without global single-flight or
/// semaphore state. Callers that already own scheduling/deduplication should use
/// this primitive directly.
pub async fn download_and_extract_to_cache(
    name: &str,
    version: &str,
    tarball_url: &str,
) -> Result<PathBuf> {
    if let Some(cache_path) = registry_cache_lookup(name, version).await? {
        return Ok(cache_path);
    }

    let bytes = download_bytes(tarball_url)
        .await
        .with_context(|| format!("Download {name}@{version} from {tarball_url}"))?;

    extract_to_cache(name, version, bytes).await
}

/// Return the registry cache path for a package version.
pub fn registry_cache_path(name: &str, version: &str) -> PathBuf {
    get_cache_dir().join(name).join(version)
}

/// Look up an already extracted registry package cache.
pub async fn registry_cache_lookup(name: &str, version: &str) -> Result<Option<PathBuf>> {
    let cache_path = registry_cache_path(name, version);
    if crate::fs::try_exists(&cache_path.join("_resolved"))
        .await
        .unwrap_or(false)
    {
        REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        Ok(Some(cache_path))
    } else {
        Ok(None)
    }
}

/// Extract already downloaded registry tarball bytes into the package cache.
pub async fn extract_to_cache(name: &str, version: &str, bytes: Bytes) -> Result<PathBuf> {
    let cache_path = registry_cache_path(name, version);

    if crate::fs::try_exists(&cache_path.join("_resolved"))
        .await
        .unwrap_or(false)
    {
        REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(cache_path);
    }

    extract_and_write(bytes, &cache_path)
        .await
        .with_context(|| format!("Extract {name}@{version} into {}", cache_path.display()))?;

    DOWNLOAD_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(cache_path)
}

/// Synchronous form of [`extract_to_cache`] for schedulers that already run
/// extraction on a CPU/disk worker.
pub fn extract_to_cache_sync(name: &str, version: &str, bytes: Bytes) -> Result<PathBuf> {
    let cache_path = registry_cache_path(name, version);

    if cache_path.join("_resolved").try_exists().unwrap_or(false) {
        REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(cache_path);
    }

    extract_and_write_sync(bytes, &cache_path)
        .with_context(|| format!("Extract {name}@{version} into {}", cache_path.display()))?;

    DOWNLOAD_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(cache_path)
}

/// Download tarball bytes with retries (network phase only).
pub async fn download_bytes(url: &str) -> Result<Bytes> {
    let retry_count = AtomicU32::new(0);
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let attempt = retry_count.fetch_add(1, Ordering::Relaxed);

            let response = DOWNLOADER_CLIENT
                .get(url)
                .send()
                .await
                .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    let bytes = response
                        .bytes()
                        .await
                        .map_err(|e| RetryableError::Temporary(format!("Stream error: {e}")))?;
                    if attempt > 0 {
                        tracing::info!("Retry succeeded on attempt {}: {url}", attempt + 1);
                    }
                    Ok(bytes)
                }
                StatusCode::NOT_FOUND => Err(RetryableError::Permanent(format!("HTTP 404: {url}"))),
                status => Err(RetryableError::Temporary(format!("HTTP {status}: {url}"))),
            }
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await
    .with_context(|| format!("Download failed after retries: {url}"))
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

    #[test]
    fn test_extract_and_write_sync() {
        let tar_gz = create_tar_gz();
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");

        extract_and_write_sync(Bytes::from(tar_gz), &dest).unwrap();

        assert!(dest.join("_resolved").exists());
        assert!(dest.join("file.txt").exists());
        let content = std::fs::read_to_string(dest.join("file.txt")).unwrap();
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
