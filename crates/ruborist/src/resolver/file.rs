//! Local `file:` dependency resolver — handles `file:./foo-1.2.3.tgz`
//! (extract) and `file:./local-pkg/` (copy).
//!
//! The cache slot is keyed on the **absolute source path** (not name/version)
//! so two unrelated packages in a monorepo with colliding names still land
//! in distinct slots. The base directory for a `file:` spec is caller-
//! supplied (see [`super::builder::process_dependency`]) because transitive
//! file: deps must resolve against the parent package's on-disk origin, not
//! its install-time path in `node_modules`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::common::{
    DedupCache, cache_slot, commit_cache_dir_atomic, dedup_init, finalize_non_registry_manifest,
};
use super::tar::{MAX_UNCOMPRESSED_BYTES, TarEntry, gzip_decompress, scan_tarball, write_entries};
use crate::model::manifest::CoreVersionManifest;
use crate::traits::registry::ResolvedPackage;

pub(crate) type FileFetchCache = DedupCache<CoreVersionManifest>;

const EXCLUDED_DIR_NAMES: &[&str] = &["node_modules", ".git"];

/// Derive the cache sub-directory name for an absolute file path.
///
/// Both ruborist (writing) and pm (lookup) call this helper to agree on the
/// same slot for a given path.
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

fn scan_directory(root: &Path) -> Result<(Vec<TarEntry>, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut manifest_blob: Option<Vec<u8>> = None;
    let mut total_bytes: u64 = 0;

    // The synthetic `package/` top-level dir keeps install-phase
    // `find_real_src` compatible with the tarball layout.
    entries.push(TarEntry {
        rel_path: PathBuf::from("package"),
        content: Vec::new(),
        mode: 0o755,
        is_dir: true,
    });

    walk_into(
        root,
        Path::new("package"),
        &mut entries,
        &mut manifest_blob,
        &mut total_bytes,
    )?;

    let manifest_blob = manifest_blob
        .ok_or_else(|| anyhow!("package.json not found in directory {}", root.display()))?;
    Ok((entries, manifest_blob))
}

fn walk_into(
    dir: &Path,
    rel_prefix: &Path,
    entries: &mut Vec<TarEntry>,
    manifest_blob: &mut Option<Vec<u8>>,
    total_bytes: &mut u64,
) -> Result<()> {
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?;
    for entry in read {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if EXCLUDED_DIR_NAMES.iter().any(|ex| *ex == name_str) {
            continue;
        }

        let src_path = entry.path();
        let rel_path = rel_prefix.join(&name);

        // Follow symlinks (fs::metadata, unlike DirEntry::file_type, does).
        // A broken symlink surfaces as an Err here rather than being read as
        // bytes. One stat covers both type and mode on unix.
        let meta = match std::fs::metadata(&src_path) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Skipping unreadable entry {} ({})", src_path.display(), e);
                continue;
            }
        };
        let file_type = meta.file_type();

        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = if file_type.is_dir() { 0o755 } else { 0o644 };

        if file_type.is_dir() {
            entries.push(TarEntry {
                rel_path: rel_path.clone(),
                content: Vec::new(),
                mode,
                is_dir: true,
            });
            walk_into(&src_path, &rel_path, entries, manifest_blob, total_bytes)?;
        } else if file_type.is_file() {
            let content = std::fs::read(&src_path)
                .with_context(|| format!("failed to read {}", src_path.display()))?;
            *total_bytes = total_bytes.saturating_add(content.len() as u64);
            if *total_bytes > MAX_UNCOMPRESSED_BYTES {
                return Err(anyhow!(
                    "file: directory {} exceeds {} MiB size limit",
                    dir.display(),
                    MAX_UNCOMPRESSED_BYTES / (1024 * 1024)
                ));
            }
            // depth == 2 because of the synthetic `package/` wrapper.
            if name_str == "package.json"
                && rel_path.components().count() == 2
                && manifest_blob.is_none()
            {
                *manifest_blob = Some(content.clone());
            }
            entries.push(TarEntry {
                rel_path,
                content,
                mode,
                is_dir: false,
            });
        } else {
            tracing::debug!("Skipping non-file entry {}", src_path.display());
        }
    }
    Ok(())
}

/// Decompress + scan a local tarball file into tar entries.
fn scan_tarball_file(path: &Path) -> Result<(Vec<TarEntry>, Vec<u8>)> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read tarball {}", path.display()))?;
    let decompressed = gzip_decompress(&bytes)?;
    scan_tarball(&decompressed)
}

fn resolve_blocking(cache_dir: &Path, abs_src: &Path) -> Result<CoreVersionManifest> {
    let metadata = std::fs::metadata(abs_src)
        .with_context(|| format!("file: target does not exist: {}", abs_src.display()))?;

    let (entries, manifest_blob) = if metadata.is_dir() {
        scan_directory(abs_src)?
    } else if metadata.is_file() {
        scan_tarball_file(abs_src)?
    } else {
        return Err(anyhow!(
            "file: target is neither a file nor a directory: {}",
            abs_src.display()
        ));
    };

    let mut manifest: CoreVersionManifest = serde_json::from_slice(&manifest_blob)
        .with_context(|| format!("failed to parse package.json from {}", abs_src.display()))?;
    let pinned_url = format!("file:{}", abs_src.to_string_lossy());
    finalize_non_registry_manifest(&mut manifest, pinned_url)?;

    let package_dir = cache_dir
        .join(&manifest.name)
        .join(file_cache_slot(abs_src));
    if package_dir.join("_resolved").exists() {
        return Ok(manifest);
    }
    commit_cache_dir_atomic(&package_dir, |stage| write_entries(&entries, stage))?;

    Ok(manifest)
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
    fn resolves_local_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg_dir = tmp.path().join("local-pkg");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::create_dir_all(pkg_dir.join("node_modules/should-be-excluded")).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            br#"{"name":"local-pkg","version":"0.0.1"}"#,
        )
        .unwrap();
        std::fs::write(pkg_dir.join("src/index.js"), b"module.exports = 42;\n").unwrap();
        std::fs::write(pkg_dir.join("node_modules/should-be-excluded/dummy"), b"x").unwrap();

        let manifest = resolve_blocking(cache.path(), &pkg_dir).unwrap();
        assert_eq!(manifest.name, "local-pkg");
        assert_eq!(manifest.version, "0.0.1");

        let expected_dir = cache
            .path()
            .join("local-pkg")
            .join(file_cache_slot(&pkg_dir));
        assert!(expected_dir.join("_resolved").exists());
        assert!(expected_dir.join("package/package.json").exists());
        assert!(expected_dir.join("package/src/index.js").exists());
        assert!(!expected_dir.join("package/node_modules").exists());
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

    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed_not_read_as_bytes() {
        // Regression: previously `DirEntry::file_type().is_file()` returned
        // false for symlinks so the walker's `is_file() || is_symlink()`
        // branch read a symlinked *directory* as raw bytes, corrupting the
        // cache. Now `fs::metadata` resolves the target first.
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg_dir = tmp.path().join("demo");
        let real_src = tmp.path().join("real-src");
        std::fs::create_dir_all(&real_src).unwrap();
        std::fs::write(real_src.join("hello.txt"), b"hi").unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            br#"{"name":"demo","version":"1.0.0"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&real_src, pkg_dir.join("linked")).unwrap();

        let manifest = resolve_blocking(cache.path(), &pkg_dir).unwrap();
        assert_eq!(manifest.name, "demo");

        let slot = cache.path().join("demo").join(file_cache_slot(&pkg_dir));
        assert!(slot.join("package/linked/hello.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlinks_are_skipped_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg_dir = tmp.path().join("demo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            br#"{"name":"demo","version":"1.0.0"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink("/nonexistent-target", pkg_dir.join("dangling")).unwrap();

        resolve_blocking(cache.path(), &pkg_dir).unwrap();
    }

    #[test]
    fn warm_cache_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();

        let pkg_dir = tmp.path().join("demo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            br#"{"name":"demo","version":"1.0.0"}"#,
        )
        .unwrap();

        resolve_blocking(cache.path(), &pkg_dir).unwrap();
        // Second run hits the `_resolved` marker short-circuit.
        resolve_blocking(cache.path(), &pkg_dir).unwrap();
    }
}
