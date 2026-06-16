//! Shared helpers for native, non-registry resolvers (git, http tarball).
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
//!   `super::http`  — caches at `<cache>/<name>/_http_<url_hash>/`

use std::path::PathBuf;

#[cfg(all(unix, any(feature = "native-git", feature = "http-tarball")))]
use std::os::unix::fs::PermissionsExt;
#[cfg(any(feature = "native-git", feature = "http-tarball"))]
use std::path::Path;

#[cfg(any(feature = "native-git", feature = "http-tarball"))]
use anyhow::Context;
use anyhow::{Result, anyhow};
#[cfg(feature = "http-tarball")]
use sha2::{Digest, Sha256};

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

// ---------------------------------------------------------------------------
// Non-registry cache-slot addressing
// ---------------------------------------------------------------------------
//
// The cache *key* for a non-registry tarball: an http(s) tarball is keyed by
// its URL, a local `file:` tarball by its absolute path. Both share the
// `cache_slot` sha256 primitive and live here — next to `commit_cache_dir_atomic`
// (the slot *commit* protocol) — so all cache-slot concerns sit in one module
// instead of the `file:` key living inside the http resolver. Consumed by
// `super::http` and `super::file`, and re-exported to pm via the lib façade.

/// `<prefix><16 hex>` from `sha256(bytes)`. Used by the two non-registry
/// cache slots (`_http_<url>`, `_file_<path>`) to stay visually distinct
/// from `<name>/<version>/` and 40-char git commit shas.
#[cfg(feature = "http-tarball")]
fn cache_slot(prefix: &str, bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(prefix.len() + 16);
    out.push_str(prefix);
    for b in &digest[..8] {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Cache slot for an http(s) tarball, keyed on its URL.
#[cfg(feature = "http-tarball")]
pub fn http_cache_slot(url: &str) -> String {
    cache_slot("_http_", url.as_bytes())
}

/// Cache slot for a local tarball keyed on its absolute path. The path is
/// lexically normalized first so BFS and install hash to the same slot.
///
/// BFS resolves the tarball from a canonical absolute path
/// (`base.join("/abs/foo.tgz")`), but install re-absolutizes a *root-relative*
/// lockfile entry (`cwd.join("../../foo.tgz")`) whenever the tarball lives
/// outside the project root. `Path::components()` collapses `.` and redundant
/// separators but **keeps `..`**, so those two spellings of the same file would
/// otherwise hash to different slots and the install-time lookup would miss.
/// [`lexically_normalize`] collapses `..` so both spellings agree.
#[cfg(feature = "http-tarball")]
pub fn file_cache_slot(abs_path: &Path) -> String {
    let normalized = lexically_normalize(abs_path);
    cache_slot("_file_", normalized.as_os_str().as_encoded_bytes())
}

/// Collapse `.` and `..` purely lexically (no filesystem access, so it works
/// for not-yet-existing paths and stays identical across BFS and install).
///
/// `..` pops the preceding normal component but never climbs past a root or
/// prefix; a leading `..` in a relative path is preserved.
///
/// # Why lexical, not `fs::canonicalize`
///
/// This matches the semantics of Node's `path.resolve`/`path.normalize` — which
/// is what npm uses to resolve `file:` specs — *not* `fs::realpath`. We only
/// need BFS and install to agree on one canonical string for the cache key, and
/// lexical collapsing achieves that without the costs of `fs::canonicalize`:
/// the latter would require the tarball to already exist (breaking optional /
/// not-yet-fetched deps), cost a syscall per package, and resolve symlinks —
/// diverging from npm and changing the key based on FS state.
///
/// # Why hand-rolled
///
/// Rust std has no lexical `..`-collapse: `Path::components()` keeps `..` (the
/// original bug), and `std::path::absolute` also deliberately keeps `..` on
/// Unix. The `path-clean` crate exists in the tree but only as a transitive dep
/// of the bundler half (swc/turbopack), not reachable from pm/ruborist, and it
/// is not Windows-prefix aware. The `Component::Prefix` arm below handles
/// Windows drive prefixes (e.g. `C:\`) so the win32 build stays correct even
/// though no `file:`-tarball e2e currently exercises that target.
#[cfg(feature = "http-tarball")]
fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut stack: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match stack.last() {
                Some(Component::Normal(_)) => {
                    stack.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => stack.push(comp),
            },
            _ => stack.push(comp),
        }
    }
    stack.iter().map(|c| c.as_os_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "http-tarball")]
    mod cache_slot {
        use std::path::{Path, PathBuf};

        use super::super::{file_cache_slot, http_cache_slot, lexically_normalize};

        #[test]
        fn http_slot_is_stable_and_url_specific() {
            let a = http_cache_slot("https://example.com/foo.tgz");
            let b = http_cache_slot("https://example.com/foo.tgz");
            let c = http_cache_slot("https://example.com/foo.tgz?v=2");
            assert_eq!(a, b);
            assert_ne!(a, c);
            assert!(a.starts_with("_http_"));
            assert_eq!(a.len(), "_http_".len() + 16);
        }

        #[test]
        fn file_slot_is_stable_across_dotdot_spellings() {
            // BFS resolves from a canonical absolute path; install re-absolutizes
            // a root-relative lockfile entry, which carries `..` when the tarball
            // lives outside the project root. Both must hash to the same slot.
            let canonical = Path::new("/tmp/pkgs/foo.tgz");
            let via_dotdot = Path::new("/tmp/pkgs/app/../../pkgs/foo.tgz");
            let via_curdir = Path::new("/tmp/pkgs/./foo.tgz");

            assert_eq!(file_cache_slot(canonical), file_cache_slot(via_dotdot));
            assert_eq!(file_cache_slot(canonical), file_cache_slot(via_curdir));
            assert!(file_cache_slot(canonical).starts_with("_file_"));

            // Distinct tarballs still get distinct slots.
            assert_ne!(
                file_cache_slot(canonical),
                file_cache_slot(Path::new("/tmp/pkgs/bar.tgz"))
            );
        }

        #[test]
        fn lexically_normalize_collapses_dotdot_without_climbing_root() {
            assert_eq!(
                lexically_normalize(Path::new("/a/b/../c")),
                PathBuf::from("/a/c")
            );
            // Excess `..` saturates at the root rather than escaping it.
            assert_eq!(
                lexically_normalize(Path::new("/a/../../../b")),
                PathBuf::from("/b")
            );
            // Leading `..` in a relative path is preserved (nothing to pop).
            assert_eq!(
                lexically_normalize(Path::new("../a/b")),
                PathBuf::from("../a/b")
            );
        }
    }

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
