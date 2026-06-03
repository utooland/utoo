//! Package cache layer: cache-path layout, seeded-slot lookups, and extracting
//! downloaded tarball bytes into the cache. The network phase lives in
//! [`super::downloader`]; the raw gzip/tar primitive lives in
//! [`super::extractor`]. This module owns the `_resolved` cache contract.

use std::path::PathBuf;

use anyhow::{Context, Result};
use bytes::Bytes;
use utoo_ruborist::http::{file_cache_slot, http_cache_slot};
use utoo_ruborist::spec::Protocol;

use super::cache::get_cache_dir;
use super::extractor::extract_and_write;

/// Outcome of materializing a registry tarball into the cache. Returned so the
/// caller (the install scheduler) keeps its own download/reuse counts instead
/// of the util layer owning global counters.
pub enum ExtractOutcome {
    /// Served from an already-extracted cache directory (no work done).
    Reused(PathBuf),
    /// Freshly extracted from downloaded bytes.
    Extracted(PathBuf),
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
        Ok(Some(cache_path))
    } else {
        Ok(None)
    }
}

/// Extract already downloaded registry tarball bytes into the package cache.
pub async fn extract_to_cache(name: &str, version: &str, bytes: Bytes) -> Result<ExtractOutcome> {
    let cache_path = registry_cache_path(name, version);

    if crate::fs::try_exists(&cache_path.join("_resolved")).await? {
        return Ok(ExtractOutcome::Reused(cache_path));
    }

    extract_and_write(bytes, &cache_path)
        .await
        .with_context(|| format!("Extract {name}@{version} into {}", cache_path.display()))?;

    Ok(ExtractOutcome::Extracted(cache_path))
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
    async fn test_extract_to_cache_extracts_then_reuses() {
        let tar_gz = create_tar_gz();
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");

        // Direct primitive: fresh extraction writes the tree + `_resolved`.
        extract_and_write(Bytes::from(tar_gz.clone()), &dest)
            .await
            .unwrap();
        assert!(dest.join("_resolved").exists());
        assert!(dest.join("file.txt").exists());
        let content = crate::fs::read_to_string(dest.join("file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");

        // Second extraction is idempotent (already resolved).
        extract_and_write(Bytes::from(tar_gz), &dest).await.unwrap();
        assert!(dest.join("file.txt").exists());
    }
}
