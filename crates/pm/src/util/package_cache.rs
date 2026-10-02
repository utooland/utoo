//! Package cache layer: cache-path layout, install-time routing, and
//! extracting tarball bytes. The network phase lives in [`super::downloader`];
//! the raw gzip/tar primitive lives in [`super::extractor`].
//!
//! Routing ([`resolve_cache_plan`]) classifies a lockfile entry by the host of
//! its `resolved` URL: git deps and trusted-registry-host tarballs use the
//! shared global store (registry content under `<cache>.utoo-v2/packages/`), while non-registry tarballs (untrusted
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
use sha2::{Digest, Sha256};

use super::cache::get_cache_dir;
use super::downloader::is_git_url;
use super::extractor::extract_and_write;
use super::user_config::is_registry_tarball;

/// Complete materialization input, retained across prefetch and authoritative
/// installation. This is deliberately internal: the public resolver event
/// and lockfile formats remain unchanged.
#[derive(Clone, Debug)]
pub(crate) struct PackageSource {
    pub name: String,
    pub version: String,
    pub tarball_url: String,
    pub integrity: Option<String>,
    pub shasum: Option<String>,
}

impl PackageSource {
    pub fn key(&self) -> String {
        format!("{}@{}:{}", self.name, self.version, self.fingerprint())
    }

    fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        for part in [
            Some(self.tarball_url.as_str()),
            self.integrity.as_deref(),
            self.shasum.as_deref(),
        ] {
            let part = part.unwrap_or("");
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        format!("{:x}", hash.finalize())
    }

    fn cache_path(&self) -> PathBuf {
        super::cache::versioned_cache_dir(&get_cache_dir())
            .join("packages")
            .join(&self.name)
            .join(&self.version)
            .join(self.fingerprint())
    }

    fn verify(&self, bytes: &[u8]) -> Result<()> {
        if let Some(integrity) = &self.integrity {
            super::integrity::verify_integrity(bytes, integrity)
        } else if let Some(shasum) = &self.shasum {
            super::integrity::verify_shasum(bytes, shasum)
        } else {
            Ok(())
        }
        .with_context(|| format!("Failed to verify {}@{}", self.name, self.version))
    }
}

/// Outcome of materializing a registry tarball into the cache. Returned so the
/// caller (the install scheduler) keeps its own download/reuse counts instead
/// of the util layer owning global counters.
pub enum ExtractOutcome {
    /// Served from an already-extracted cache directory (no work done).
    Reused(PathBuf),
    /// Freshly extracted from downloaded bytes.
    Extracted(PathBuf),
}

/// Recover the exact Git checkout recorded by the lockfile, including on a
/// fresh machine. A branch/tag is not a lock and must never silently move.
async fn ensure_locked_git(
    name: &str,
    tarball_url: &str,
    clones: &utoo_ruborist::git::GitCloneCache,
) -> Result<PathBuf> {
    let (url, commit) = tarball_url
        .rsplit_once('#')
        .context("locked Git dependency has no commit")?;
    anyhow::ensure!(
        (commit.len() == 40 || commit.len() == 64) && commit.bytes().all(|c| c.is_ascii_hexdigit()),
        "locked Git dependency must reference a full commit: {tarball_url}"
    );
    let cached =
        utoo_ruborist::git::ensure_repo_cached(&get_cache_dir(), url, Some(commit), name, clones)
            .await?;
    anyhow::ensure!(
        cached
            .resolved_url
            .rsplit_once('#')
            .is_some_and(|(_, sha)| sha.eq_ignore_ascii_case(commit)),
        "Git checkout does not match locked commit {commit}"
    );
    Ok(cached.path.clone())
}

/// How the install phase should materialize a lockfile entry, decided purely
/// by classifying its `resolved` URL by host/scheme (the lockfile stores no
/// source tag — see [`is_registry_tarball`]).
pub enum CachePlan {
    /// A git dep, recovered at its locked commit in the global cache at
    /// `<cache>/<name>/<commit_sha>/`; clone from there.
    GitCache(PathBuf),
    /// A registry-host tarball: use the v2 `<name>/<version>/<source-and-digest>`
    /// slot, then clone from there. Legacy slots are treated as misses.
    RegistryDownload,
    /// A non-registry tarball (http(s) remote or local `file:`): fetch/read the
    /// tarball and extract it **directly** into the package's `node_modules`
    /// target. These never enter the global cache.
    DirectExtract,
}

/// Classify a lockfile entry's `resolved` URL into a [`CachePlan`].
///
/// Routing is by host/scheme only:
/// - `git+…` / git URLs → [`CachePlan::GitCache`] (recover a missing checkout
///   from the locked URL and commit).
/// - a trusted registry-host tarball → [`CachePlan::RegistryDownload`].
/// - anything else (untrusted https or `file:`) → [`CachePlan::DirectExtract`].
pub async fn resolve_cache_plan(
    name: &str,
    tarball_url: &str,
    clones: &utoo_ruborist::git::GitCloneCache,
) -> Result<CachePlan> {
    if is_git_url(tarball_url) {
        return ensure_locked_git(name, tarball_url, clones)
            .await
            .map(CachePlan::GitCache);
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
pub async fn extract_non_registry_to_target(source: &PackageSource, target: &Path) -> Result<()> {
    let tarball_url = &source.tarball_url;
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

    source.verify(&bytes)?;
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || utoo_ruborist::tar::extract_tarball_to_dir(&bytes, &target))
        .await
        .context("direct-extract task failed")?
        .with_context(|| format!("failed to extract tarball {tarball_url}"))
}

/// Look up an already extracted registry package cache.
pub async fn registry_cache_lookup(source: &PackageSource) -> Result<Option<PathBuf>> {
    let cache_path = source.cache_path();
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
pub async fn extract_to_cache(source: &PackageSource, bytes: Bytes) -> Result<ExtractOutcome> {
    source.verify(&bytes)?;
    let cache_path = source.cache_path();

    if crate::fs::try_exists(&cache_path.join("_resolved")).await? {
        return Ok(ExtractOutcome::Reused(cache_path));
    }

    extract_and_write(bytes, &cache_path)
        .await
        .with_context(|| format!("Extract {} into {}", source.name, cache_path.display()))?;

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
