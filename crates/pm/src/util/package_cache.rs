//! Package cache layer: registry cache-path layout, seeded-slot resolution, and
//! extracting downloaded tarball bytes into the cache. The network phase lives
//! in [`super::downloader`]; the raw gzip/tar primitive lives in
//! [`super::extractor`]. This module owns the `_resolved` cache contract.
//!
//! NOTE: these primitives are staged for the install scheduler landing in the
//! follow-up PR; until then they have no in-tree consumer, hence the
//! module-level `allow(dead_code)`. The follow-up PR wires them into the
//! scheduler and removes this allow.
#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use bytes::Bytes;
use utoo_ruborist::spec::Protocol;

use super::cache::get_cache_dir;
use super::downloader::{
    download_bytes, file_cache_lookup, git_cache_lookup, http_tarball_cache_lookup,
};
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

impl ExtractOutcome {
    pub fn into_path(self) -> PathBuf {
        match self {
            ExtractOutcome::Reused(p) | ExtractOutcome::Extracted(p) => p,
        }
    }
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

    if crate::fs::try_exists(&cache_path.join("_resolved"))
        .await
        .unwrap_or(false)
    {
        return Ok(ExtractOutcome::Reused(cache_path));
    }

    extract_and_write(bytes, &cache_path)
        .await
        .with_context(|| format!("Extract {name}@{version} into {}", cache_path.display()))?;

    Ok(ExtractOutcome::Extracted(cache_path))
}

/// Download and extract a registry tarball without global single-flight or
/// semaphore state. Callers that own scheduling/deduplication use the network +
/// extract primitives directly.
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

    Ok(extract_to_cache(name, version, bytes).await?.into_path())
}
