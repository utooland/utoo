//! Local `file:` dependency resolver — npm-compatible semantics:
//!
//! - `file:./foo-1.2.3.tgz` — tarball; extract into the shared cache slot
//!   (via [`super::tar::commit_tarball_bytes`], same machinery as http).
//! - `file:./local-dir/`    — directory; install as a symlink. The
//!   directory's absolute path travels through `manifest.dist.link_target`
//!   so the lockfile serializer emits `link: true` + relative `resolved`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::common::{DedupCache, cache_slot, dedup_init, finalize_non_registry_manifest};
use super::tar::commit_tarball_bytes;
use crate::model::manifest::CoreVersionManifest;
use crate::traits::registry::ResolvedPackage;

pub(crate) type FileFetchCache = DedupCache<CoreVersionManifest>;

/// Derive the cache sub-directory name for an absolute tarball path.
///
/// Only tarball `file:` deps touch the cache — directory deps symlink in
/// place. Shared between ruborist (writer) and pm (install-time lookup).
pub fn file_cache_slot(abs_path: &Path) -> String {
    cache_slot("_file_", abs_path.as_os_str().as_encoded_bytes())
}

/// Textual `base_dir.join(spec)` normalization that collapses `.` / `..`
/// without touching the filesystem.
///
/// We intentionally avoid `canonicalize` here so a missing target surfaces
/// as "file: target does not exist" rather than an opaque kernel error.
fn normalize_path(base_dir: &Path, spec: &str) -> PathBuf {
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

/// Directory deps: read `package.json` only — installation is a symlink,
/// not a copy. `dist.link_target` carries the absolute source path; the
/// lockfile serializer turns it into `link: true` + `resolved: <relative>`.
fn resolve_dir_blocking(abs_src: &Path) -> Result<CoreVersionManifest> {
    let manifest_path = abs_src.join("package.json");
    let blob = std::fs::read(&manifest_path)
        .with_context(|| format!("package.json not found in directory {}", abs_src.display()))?;
    let mut manifest: CoreVersionManifest = serde_json::from_slice(&blob)
        .with_context(|| format!("failed to parse package.json from {}", abs_src.display()))?;
    // Synthesize the manifest like git/http but mark this as a symlink
    // dep instead of staging anything to the cache.
    let pinned_url = format!("file:{}", abs_src.to_string_lossy());
    finalize_non_registry_manifest(&mut manifest, pinned_url)?;
    manifest.dist.link_target = Some(abs_src.to_path_buf());
    Ok(manifest)
}

fn resolve_tarball_blocking(cache_dir: &Path, abs_src: &Path) -> Result<CoreVersionManifest> {
    let bytes = std::fs::read(abs_src)
        .with_context(|| format!("failed to read tarball {}", abs_src.display()))?;
    let pinned_url = format!("file:{}", abs_src.to_string_lossy());
    commit_tarball_bytes(cache_dir, &bytes, pinned_url, &file_cache_slot(abs_src))
}

fn resolve_blocking(cache_dir: &Path, abs_src: &Path) -> Result<CoreVersionManifest> {
    let metadata = std::fs::metadata(abs_src)
        .with_context(|| format!("file: target does not exist: {}", abs_src.display()))?;
    if metadata.is_dir() {
        resolve_dir_blocking(abs_src)
    } else if metadata.is_file() {
        resolve_tarball_blocking(cache_dir, abs_src)
    } else {
        Err(anyhow!(
            "file: target is neither a file nor a directory: {}",
            abs_src.display()
        ))
    }
}

/// Resolve a `file:` spec to a [`ResolvedPackage`] and seed the cache.
///
/// - `base_dir` — absolute directory against which `path_spec` is resolved.
/// - `path_spec` — the raw path from the spec (the substring *after* `file:`).
///
/// The resolved absolute path is normalized textually (`.`/`..` collapsed)
/// rather than canonicalized, so clearer error messages survive even when
/// the target is missing.
pub(crate) async fn resolve_file_dep(
    cache_dir: Option<&Path>,
    base_dir: &Path,
    path_spec: &str,
    fetch_cache: &FileFetchCache,
) -> Result<ResolvedPackage> {
    let cache_dir =
        cache_dir.ok_or_else(|| anyhow!("cache_dir required for file dependency resolution"))?;
    if !base_dir.is_absolute() {
        return Err(anyhow!(
            "file: base_dir must be absolute, got {}",
            base_dir.display()
        ));
    }

    let abs_src = normalize_path(base_dir, path_spec);
    let abs_src_owned = abs_src.clone();
    let cache_dir_owned = cache_dir.to_path_buf();
    let key = format!("file:{}", abs_src.to_string_lossy());

    let manifest = dedup_init(fetch_cache, key, move || async move {
        tokio::task::spawn_blocking(move || {
            resolve_blocking(&cache_dir_owned, &abs_src_owned).map(Arc::new)
        })
        .await
        .context("file resolver task failed")?
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
        let expected_tarball = format!("file:{}", tarball_path.to_string_lossy());
        assert_eq!(
            manifest.dist.tarball.as_deref(),
            Some(expected_tarball.as_str())
        );

        let expected_dir = cache
            .path()
            .join("demo")
            .join(file_cache_slot(&tarball_path));
        assert!(expected_dir.join("_resolved").exists());
        assert!(expected_dir.join("package").join("package.json").exists());
    }

    #[test]
    fn directory_dep_sets_link_target_and_skips_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg_dir = tmp.path().join("local-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            br#"{"name":"local-pkg","version":"0.0.1"}"#,
        )
        .unwrap();
        std::fs::write(pkg_dir.join("index.js"), b"module.exports = 42;\n").unwrap();

        let manifest = resolve_blocking(cache.path(), &pkg_dir).unwrap();
        assert_eq!(manifest.name, "local-pkg");
        assert_eq!(manifest.version, "0.0.1");
        // The directory IS the install target — symlink at install time.
        assert_eq!(
            manifest.dist.link_target.as_deref(),
            Some(pkg_dir.as_path())
        );
        assert!(
            !cache.path().join("local-pkg").exists(),
            "directory dep must not populate the cache slot"
        );
    }

    #[test]
    fn directory_missing_package_json_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg_dir = tmp.path().join("empty");
        std::fs::create_dir_all(&pkg_dir).unwrap();

        let err = resolve_blocking(cache.path(), &pkg_dir).unwrap_err();
        assert!(err.to_string().contains("package.json not found"));
    }

    #[test]
    fn missing_target_errors_clearly() {
        let cache = tempfile::tempdir().unwrap();
        let err = resolve_blocking(cache.path(), Path::new("/does/not/exist/pkg.tgz")).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
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
