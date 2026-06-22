//! Shared helpers for native, non-registry resolvers (git, http/file tarball).
//!
//! - [`DedupCache<T>`] — session-scoped single-flight cache (alias for
//!   [`OnceMap`](crate::util::OnceMap)) so concurrent fetches of the same key
//!   share one underlying request.
//! - [`validate_package_name`] — path-traversal guard for cache paths
//! - [`finalize_non_registry_manifest`] — empty-version default, `dist`,
//!   `has_install_script`
//! - [`commit_cache_dir_atomic`] — staging-dir → final-path with `_resolved`
//!   marker and ENOTEMPTY race handling
//!
//! Consumers:
//!   `super::git`   — caches at `<cache>/<name>/<commit_sha>/`
//!   `super::http`/`super::file` — read only the tarball manifest in BFS; the
//!     tarball *content* is materialized directly into `node_modules` at
//!     install time and never enters the global cache.

use std::path::PathBuf;

#[cfg(all(unix, any(feature = "native-git", feature = "http-tarball")))]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(feature = "native-git", feature = "http-tarball"))]
use std::path::Path;

#[cfg(any(feature = "native-git", feature = "http-tarball"))]
use anyhow::Context;
use anyhow::{Result, anyhow};

use crate::model::manifest::{CoreVersionManifest, Dist};
use crate::util::OnceMap;

/// Session-scoped dedup cache keyed by canonical URL (+ optional ref).
///
/// Concurrent requests for the same key share a single [`OnceMap`] entry,
/// so only the first caller performs the fetch and the rest await its result.
/// On error, the entry is removed so subsequent calls retry.
pub type DedupCache<T> = OnceMap<String, T>;

/// Reject package names that contain path-traversal components
/// (`..`, absolute roots) before using them in `cache_dir.join(name)`.
pub fn validate_package_name(name: &str) -> Result<()> {
    let name_path = PathBuf::from(name);
    if name_path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Err(anyhow!(
            "Suspicious package name '{}' — refusing to use for cache path",
            name
        ));
    }
    Ok(())
}

/// Fill in manifest fields the npm registry would normally supply but that
/// non-registry packages must synthesize from `package.json`:
///
/// - errors when `name` is empty
/// - defaults `version` to `"0.0.0"` when missing (npm tolerates this)
/// - validates the manifest `name` for path traversal
/// - sets `dist.tarball` to `pinned_url` (clears integrity)
/// - sets `has_install_script` based on `scripts` containing
///   `preinstall`/`install`/`postinstall`
pub fn finalize_non_registry_manifest(
    manifest: &mut CoreVersionManifest,
    pinned_url: String,
) -> Result<()> {
    if manifest.name.is_empty() {
        return Err(anyhow!("package.json missing 'name' field"));
    }
    if manifest.version.is_empty() {
        tracing::debug!(
            "package.json for '{}' missing 'version'; defaulting to 0.0.0",
            manifest.name
        );
        manifest.version = "0.0.0".to_string();
    }
    validate_package_name(&manifest.name)?;

    manifest.dist = Dist {
        tarball: Some(pinned_url),
        integrity: None,
        ..Default::default()
    };
    manifest.has_install_script = Some(manifest.scripts.as_ref().is_some_and(|s| {
        s.contains_key("preinstall") || s.contains_key("install") || s.contains_key("postinstall")
    }));

    Ok(())
}

/// Atomically commit a populated staging directory to `package_dir`.
///
/// This is the single durability contract for every `~/.cache/nm/` slot
/// kind (registry `<name>/<version>`, git `<name>/<sha>`, http
/// `<name>/_http_<hash>`, file `<name>/_file_<hash>`): a slot becomes
/// visible only via atomic rename of a fully-written staging dir that
/// already contains the `_resolved` marker. A `kill -9` at any point
/// leaves either no slot or a complete slot — never a partial tree a
/// later run could mistake for a cache hit. pm's install-phase extractor
/// (`crates/pm/src/util/extractor.rs`) commits through this same helper.
///
/// The `write` callback receives a fresh empty staging directory; it must
/// write all package contents there (but **not** the `_resolved` marker —
/// this helper writes it afterwards so the final rename is atomic: any
/// directory at `package_dir` containing `_resolved` is fully populated).
///
/// If another process wins the race (EEXIST / ENOTEMPTY on rename), the
/// staging dir is discarded silently and the winner's committed dir is
/// left untouched.
#[cfg(any(feature = "native-git", feature = "http-tarball"))]
pub fn commit_cache_dir_atomic<F>(package_dir: &Path, write: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    let parent_dir = package_dir
        .parent()
        .ok_or_else(|| anyhow!("package_dir has no parent: {}", package_dir.display()))?;
    std::fs::create_dir_all(parent_dir)
        .with_context(|| format!("failed to create cache parent {}", parent_dir.display()))?;

    let tmp_dir = tempfile::tempdir_in(parent_dir).context("failed to create staging directory")?;

    write(tmp_dir.path())?;
    std::fs::write(tmp_dir.path().join("_resolved"), "")
        .context("failed to write _resolved marker")?;

    // tempfile creates staging dirs 0o700; widen the slot root to 0o755 so
    // the published slot matches a plain mkdir under the default umask and
    // stays traversable in shared-cache setups.
    #[cfg(unix)]
    if let Err(e) = std::fs::set_permissions(tmp_dir.path(), std::fs::Permissions::from_mode(0o755))
    {
        tracing::warn!(
            "failed to widen staging dir mode for {}: {e}",
            package_dir.display()
        );
    }

    // `keep()` consumes the TempDir without deleting it on success.
    let tmp_path = tmp_dir.keep();
    match std::fs::rename(&tmp_path, package_dir) {
        Ok(()) => Ok(()),
        // On Linux/macOS, rename(2) over an existing non-empty directory
        // yields ENOTEMPTY rather than EEXIST, so we check both.
        Err(e)
            if e.kind() == std::io::ErrorKind::AlreadyExists
                || e.raw_os_error() == Some(libc::ENOTEMPTY) =>
        {
            let _ = std::fs::remove_dir_all(&tmp_path);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_path);
            Err(anyhow!(
                "failed to commit cache dir {}: {e}",
                package_dir.display()
            ))
        }
    }
}

/// Look up (or create) the dedup entry for `key`, then run `init` under it.
///
/// Single-flight wrapper over [`OnceMap::get_or_try_init`]: concurrent callers
/// for the same key share one `init` invocation; on `Err` the entry is removed
/// so subsequent calls retry.
pub async fn dedup_init<T, F, Fut>(
    cache: &DedupCache<T>,
    key: String,
    init: F,
) -> Result<std::sync::Arc<T>>
where
    T: Send + Sync + 'static,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    cache
        .get_or_try_init::<anyhow::Error, _, _>(key, init)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_package_names() {
        assert!(validate_package_name("../evil").is_err());
        assert!(validate_package_name("/etc/passwd").is_err());
        assert!(validate_package_name("foo/../bar").is_err());
        assert!(validate_package_name("@scope/pkg").is_ok());
        assert!(validate_package_name("lodash").is_ok());
    }

    #[test]
    fn finalize_sets_dist_and_install_script() {
        let mut m = CoreVersionManifest {
            name: "demo".into(),
            version: "1.0.0".into(),
            scripts: Some(
                [("postinstall".to_string(), "node bla".to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        };
        finalize_non_registry_manifest(&mut m, "https://example.com/demo.tgz".into()).unwrap();
        assert_eq!(
            m.dist.tarball.as_deref(),
            Some("https://example.com/demo.tgz")
        );
        assert_eq!(m.has_install_script, Some(true));
    }

    #[test]
    fn finalize_defaults_empty_version() {
        let mut m = CoreVersionManifest {
            name: "demo".into(),
            version: String::new(),
            ..Default::default()
        };
        finalize_non_registry_manifest(&mut m, "u".into()).unwrap();
        assert_eq!(m.version, "0.0.0");
        assert_eq!(m.has_install_script, Some(false));
    }

    #[test]
    fn finalize_errors_on_empty_name() {
        let mut m = CoreVersionManifest::default();
        assert!(finalize_non_registry_manifest(&mut m, "u".into()).is_err());
    }

    #[cfg(any(feature = "native-git", feature = "http-tarball"))]
    #[test]
    fn commit_atomic_persists_resolved_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("pkg").join("1.0.0");
        commit_cache_dir_atomic(&target, |stage| {
            std::fs::write(stage.join("hello.txt"), b"hi")?;
            Ok(())
        })
        .unwrap();
        assert!(target.join("_resolved").exists());
        assert_eq!(std::fs::read(target.join("hello.txt")).unwrap(), b"hi");
    }

    #[cfg(any(feature = "native-git", feature = "http-tarball"))]
    #[test]
    fn commit_atomic_race_loser_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("pkg").join("1.0.0");
        commit_cache_dir_atomic(&target, |stage| {
            std::fs::write(stage.join("first.txt"), b"1")?;
            Ok(())
        })
        .unwrap();
        // Second commit: target already exists populated → race-loser path.
        commit_cache_dir_atomic(&target, |stage| {
            std::fs::write(stage.join("second.txt"), b"2")?;
            Ok(())
        })
        .unwrap();
        // Winner's files remain; loser's files are discarded.
        assert!(target.join("first.txt").exists());
        assert!(!target.join("second.txt").exists());
    }
}
