//! Local `file:./pkg.tgz` **tarball** resolver — thin wrapper over
//! [`super::tar::commit_tarball_bytes`], identical to http except the
//! bytes come from disk instead of the network.
//!
//! `file:<dir>` dependencies are *not* handled here; the graph builder
//! recognizes them and adds a [`NodeType::Link`](crate::graph::EdgeType)
//! node directly (same shape as a workspace link), so they never touch
//! the cache.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::common::{DedupCache, cache_slot, dedup_init};
use super::tar::commit_tarball_bytes;
use crate::model::manifest::CoreVersionManifest;
use crate::traits::registry::ResolvedPackage;

pub(crate) type FileFetchCache = DedupCache<CoreVersionManifest>;

/// Derive the cache sub-directory name for an absolute tarball path.
///
/// Shared between ruborist (writer) and pm (install-time lookup).
pub fn file_cache_slot(abs_path: &Path) -> String {
    cache_slot("_file_", abs_path.as_os_str().as_encoded_bytes())
}

/// Textual `base_dir.join(spec)` normalization that collapses `.` / `..`
/// without touching the filesystem. We intentionally avoid `canonicalize`
/// here so a missing target surfaces as "target does not exist" rather
/// than an opaque kernel error.
pub(crate) fn normalize_path(base_dir: &Path, spec: &str) -> PathBuf {
    let raw = PathBuf::from(spec);
    let candidate = if raw.is_absolute() {
        raw
    } else {
        base_dir.join(raw)
    };
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn resolve_blocking(cache_dir: &Path, abs_src: &Path) -> Result<CoreVersionManifest> {
    let bytes = std::fs::read(abs_src)
        .with_context(|| format!("failed to read tarball {}", abs_src.display()))?;
    let pinned_url = format!("file:{}", abs_src.to_string_lossy());
    commit_tarball_bytes(cache_dir, &bytes, pinned_url, &file_cache_slot(abs_src))
}

/// Resolve a `file:<tarball>` spec into a [`ResolvedPackage`], seeding the
/// cache slot. Caller must have verified `abs_src` is a regular file.
pub(crate) async fn resolve_file_tarball_dep(
    cache_dir: Option<&Path>,
    abs_src: PathBuf,
    fetch_cache: &FileFetchCache,
) -> Result<ResolvedPackage> {
    let cache_dir = cache_dir
        .ok_or_else(|| anyhow!("cache_dir required for file tarball resolution"))?
        .to_path_buf();
    let key = format!("file:{}", abs_src.to_string_lossy());

    let manifest = dedup_init(fetch_cache, key, move || async move {
        tokio::task::spawn_blocking(move || resolve_blocking(&cache_dir, &abs_src).map(Arc::new))
            .await
            .context("file tarball resolver task failed")?
    })
    .await?;

    Ok(ResolvedPackage {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn make_targz(entries: &[(&str, &[u8])]) -> Bytes {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            for (path, body) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append(&header, *body).unwrap();
            }
            tar.finish().unwrap();
        }
        let mut compressor = libdeflater::Compressor::new(libdeflater::CompressionLvl::default());
        let mut compressed = vec![0u8; compressor.gzip_compress_bound(tar_data.len())];
        let n = compressor
            .gzip_compress(&tar_data, &mut compressed)
            .unwrap();
        compressed.truncate(n);
        Bytes::from(compressed)
    }

    #[test]
    fn normalize_path_collapses_dot_segments() {
        let base = Path::new("/project");
        assert_eq!(
            normalize_path(base, "./local-pkg"),
            PathBuf::from("/project/local-pkg")
        );
        assert_eq!(
            normalize_path(base, "../sibling/pkg.tgz"),
            PathBuf::from("/sibling/pkg.tgz")
        );
        assert_eq!(
            normalize_path(base, "/abs/path"),
            PathBuf::from("/abs/path")
        );
        assert_eq!(
            normalize_path(base, "deep/../x.tgz"),
            PathBuf::from("/project/x.tgz")
        );
    }

    #[test]
    fn slot_is_path_specific_and_stable() {
        let a = file_cache_slot(Path::new("/foo/bar.tgz"));
        let b = file_cache_slot(Path::new("/foo/bar.tgz"));
        let c = file_cache_slot(Path::new("/foo/baz.tgz"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("_file_"));
        assert_eq!(a.len(), "_file_".len() + 16);
    }

    #[test]
    fn resolves_local_tarball() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg = br#"{"name":"demo","version":"1.2.3"}"#;
        let tarball_bytes = make_targz(&[("package/package.json", pkg)]);
        let tarball_path = tmp.path().join("demo-1.2.3.tgz");
        std::fs::write(&tarball_path, &tarball_bytes).unwrap();

        let manifest = resolve_blocking(cache.path(), &tarball_path).unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.2.3");

        let expected_dir = cache
            .path()
            .join("demo")
            .join(file_cache_slot(&tarball_path));
        assert!(expected_dir.join("_resolved").exists());
        assert!(expected_dir.join("package/package.json").exists());
    }

    #[test]
    fn missing_target_errors_clearly() {
        let cache = tempfile::tempdir().unwrap();
        let err = resolve_blocking(cache.path(), Path::new("/does/not/exist/pkg.tgz")).unwrap_err();
        assert!(err.to_string().contains("failed to read tarball"));
    }

    #[test]
    fn tarball_dep_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg = br#"{"name":"demo","version":"1.0.0"}"#;
        let tarball_path = tmp.path().join("demo.tgz");
        std::fs::write(&tarball_path, make_targz(&[("package/package.json", pkg)])).unwrap();

        resolve_blocking(cache.path(), &tarball_path).unwrap();
        // Second call hits the `_resolved` marker short-circuit inside
        // `commit_tarball_bytes` rather than re-extracting.
        resolve_blocking(cache.path(), &tarball_path).unwrap();
    }
}
