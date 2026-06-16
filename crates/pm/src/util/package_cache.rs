//! Package cache layer: cache-path layout, install-time routing, and
//! extracting tarball bytes. The network phase lives in [`super::downloader`];
//! the raw gzip/tar primitive lives in [`super::extractor`].
//!
//! Routing ([`resolve_cache_plan`]) classifies a lockfile entry by the host of
//! its `resolved` URL: git deps and trusted-registry-host tarballs use the
//! shared global `~/.cache/nm/` store, while non-registry tarballs (untrusted
//! https + local `file:`) are materialized **directly** into `node_modules`
//! ([`extract_non_registry_to_target`]) and never enter the cache.
//!
//! Cache lookups key on the `_resolved` marker, whose contract is shared with
//! ruborist's BFS-seeded git slots (`resolver/common.rs`): every cache slot
//! becomes visible only via atomic rename of a fully-written staging dir that
//! already contains `_resolved`, so any slot with the marker is complete.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bytes::Bytes;

use super::cache::get_cache_dir;
use super::downloader::is_git_url;
use super::extractor::extract_and_write;
use super::user_config::is_registry_tarball;

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

/// How the install phase should materialize a lockfile entry, decided purely
/// by classifying its `resolved` URL by host/scheme (the lockfile stores no
/// source tag — see [`is_registry_tarball`]).
pub enum CachePlan {
    /// A git dep, already cloned into the global cache during BFS at
    /// `<cache>/<name>/<commit_sha>/`; clone from there.
    GitCache(PathBuf),
    /// A registry-host tarball: download into the global `<name>/<version>`
    /// slot, then clone from there (the existing registry pipeline).
    RegistryDownload,
    /// A non-registry tarball (http(s) remote or local `file:`): fetch/read the
    /// tarball and extract it **directly** into the package's `node_modules`
    /// target. These never enter the global cache.
    DirectExtract,
}

/// Classify a lockfile entry's `resolved` URL into a [`CachePlan`].
///
/// Routing is by host/scheme only:
/// - `git+…` / git URLs → [`CachePlan::GitCache`] (errors if the BFS-seeded
///   git slot is missing, which would indicate a corrupt lockfile/cache).
/// - a trusted registry-host tarball → [`CachePlan::RegistryDownload`].
/// - anything else (untrusted https or `file:`) → [`CachePlan::DirectExtract`].
pub async fn resolve_cache_plan(name: &str, version: &str, tarball_url: &str) -> Result<CachePlan> {
    if is_git_url(tarball_url) {
        return git_cache_lookup(name, version, tarball_url)
            .await
            .map(CachePlan::GitCache)
            .ok_or_else(|| anyhow::anyhow!("git cache not found for {name}@{version}"));
    }
    if is_registry_tarball(tarball_url) {
        return Ok(CachePlan::RegistryDownload);
    }
    Ok(CachePlan::DirectExtract)
}

/// Materialize a non-registry tarball ([`CachePlan::DirectExtract`]) straight
/// into `target` (the package's `node_modules` directory), bypassing the global
/// cache entirely.
///
/// `file:<abs>` tarballs are read from disk; everything else is fetched over
/// http(s) (with a registry auth token only when the host warrants one). The
/// tarball is then extracted via [`extract_tarball_to_dir`], which strips the
/// npm `package/` wrapper and writes no `_resolved` marker.
pub async fn extract_non_registry_to_target(tarball_url: &str, target: &Path) -> Result<()> {
    let bytes: Bytes = if let Some(abs) = tarball_url.strip_prefix("file:") {
        let abs = abs.to_string();
        tokio::task::spawn_blocking(move || std::fs::read(&abs).map(Bytes::from))
            .await
            .context("file tarball read task failed")?
            .with_context(|| format!("failed to read tarball {tarball_url}"))?
    } else {
        let token = crate::service::auth::token_for_url(tarball_url).await;
        super::downloader::download_bytes(tarball_url, token.as_deref())
            .await
            .with_context(|| format!("failed to download tarball {tarball_url}"))?
    };

    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || utoo_ruborist::tar::extract_tarball_to_dir(&bytes, &target))
        .await
        .context("direct-extract task failed")?
        .with_context(|| format!("failed to extract tarball {tarball_url}"))
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
