use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use tokio::sync::Semaphore;
use tokio_retry::RetryIf;
use utoo_ruborist::http::{file_cache_slot, http_cache_slot};
use utoo_ruborist::spec::Protocol;
use utoo_ruborist::util::OnceMap;

use super::cache::get_cache_dir;
use super::extractor::extract_and_write;
use super::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};
use super::user_config::get_manifests_concurrency_limit_sync;

// Global downloader client - no pool limit, concurrency controlled by OnceMap
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

/// Global download cache shared between pipeline and install phases.
/// Key: "name@version", Value: cache path.
static DOWNLOAD_CACHE: Lazy<OnceMap<String, PathBuf>> = Lazy::new(OnceMap::new);

/// Semaphore controlling concurrent download count.
static DOWNLOAD_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

/// Prefetch back-pressure gate, sized smaller than [`DOWNLOAD_SEMAPHORE`].
///
/// `spawn_tarball_prefetch` fan-outs N spawn calls on entry; without
/// this gate, the first ~N tasks acquire `DOWNLOAD_SEMAPHORE` slots
/// before `install_packages` even spawns its first layer, so install's
/// clone tasks queue behind every prefetch task. p3_cold_install on
/// CI bench-phases-linux saw σ jump from 1.56s → 3.93s as a result.
///
/// Capping prefetch at 16 leaves `64 - 16 = 48` slots permanently
/// available to the install path; the OnceMap dedup still ensures
/// install's later resolve hits the prefetched future on completion.
static PREFETCH_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
const PREFETCH_BACKLOG_LIMIT: usize = 16;

static DOWNLOAD_COUNT: AtomicUsize = AtomicUsize::new(0);
static REUSE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Process-global counters for tarball outcomes, matching pnpm's
/// vocabulary. Each unique `(name, version)` pair lands in exactly one
/// bucket thanks to `DOWNLOAD_CACHE`'s `OnceMap` dedup; git/file/link
/// packages bypass this path and are not counted in either bucket.
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

/// Resolve the local cache path for a package, downloading if necessary.
///
/// The cloner calls this with a fully-qualified URL (never a relative
/// path) — the install loop is responsible for any lockfile-format
/// rewriting before we get here.
pub async fn resolve_cache_path(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    match tarball_url.parse::<Protocol>() {
        Ok(Protocol::Git) => git_cache_lookup(name, version, tarball_url).await,
        Ok(Protocol::File) => file_cache_lookup(name, tarball_url).await,
        // Otherwise try the URL-hashed http slot BFS may have seeded,
        // then fall through to the registry `<name>/<version>/` path.
        _ => match http_tarball_cache_lookup(name, tarball_url).await {
            Some(p) => Some(p),
            None => download_to_cache(name, version, tarball_url).await,
        },
    }
}

/// Prefetch variant: same as [`download_to_cache`] but bounded by
/// [`PREFETCH_SEMAPHORE`] so a fan-out batch can't starve the install
/// path of [`DOWNLOAD_SEMAPHORE`] slots.
///
/// On a warm-cache hit there's nothing to gate (the OnceMap closure
/// short-circuits before acquiring [`DOWNLOAD_SEMAPHORE`]); the
/// prefetch gate is only acquired around the actual `download_to_cache`
/// call, so warm tarballs return immediately without consuming
/// prefetch slots.
pub async fn prefetch_to_cache(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    // Quick warm-cache short-circuit before paying the prefetch gate.
    let cache_path = get_cache_dir().join(name).join(version);
    if crate::fs::try_exists(&cache_path.join("_resolved"))
        .await
        .unwrap_or(false)
    {
        return Some(cache_path);
    }

    let permit = PREFETCH_SEMAPHORE
        .get_or_init(|| Semaphore::new(PREFETCH_BACKLOG_LIMIT))
        .acquire()
        .await
        .ok()?;
    let result = download_to_cache(name, version, tarball_url).await;
    drop(permit);
    result
}

/// Download a registry tarball to the global cache directory, returning the cache path.
///
/// Uses `OnceMap` to deduplicate: the same `name@version` is only downloaded once,
/// even when called concurrently from multiple tasks (pipeline workers, install phase, etc.).
///
/// For git-resolved packages, use [`resolve_cache_path`] instead.
pub async fn download_to_cache(name: &str, version: &str, tarball_url: &str) -> Option<PathBuf> {
    let key = format!("{}@{}", name, version);

    // Hot fast-path: when the prefetch (or a sibling resolve) already
    // populated the OnceMap entry, skip the per-call string allocs and
    // future construction by reusing the cached `Arc<PathBuf>`. The
    // pipeline path drives `download_to_cache` for every BFS-resolved
    // package, and install-loop calls usually arrive after — most hit
    // a `Done` slot.
    if DOWNLOAD_CACHE.is_done(&key) {
        return DOWNLOAD_CACHE
            .get_or_init(key, || async { None })
            .await
            .as_deref()
            .cloned();
    }

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
                REUSE_COUNT.fetch_add(1, Ordering::Relaxed);
                return Some(cache_path);
            }

            // Download (semaphore controlled)
            let semaphore = DOWNLOAD_SEMAPHORE
                .get_or_init(|| Semaphore::new(get_manifests_concurrency_limit_sync()));
            let _permit = semaphore.acquire().await.ok()?;
            let bytes = download_bytes(&tarball_url)
                .await
                .inspect_err(|e| {
                    tracing::warn!(
                        "Download {}@{} from {}: {:#}",
                        name,
                        version,
                        tarball_url,
                        e
                    )
                })
                .ok()?;

            // Extract
            extract_and_write(bytes, &cache_path)
                .await
                .inspect_err(|e| {
                    tracing::warn!(
                        "Extract {}@{} into {}: {:#}",
                        name,
                        version,
                        cache_path.display(),
                        e
                    )
                })
                .ok()?;

            DOWNLOAD_COUNT.fetch_add(1, Ordering::Relaxed);
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
