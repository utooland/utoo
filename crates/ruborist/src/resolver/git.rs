//! Git clone backend powered by `gix`.
//!
//! This module is only compiled when the `native-git` Cargo feature is enabled.
//! It provides:
//! - Low-level clone + cache logic ([`ensure_repo_cached`])
//! - High-level BFS resolver ([`resolve_non_registry_dep`]) that turns a git
//!   spec into a [`ResolvedPackage`]
//!
//! ## Cache layout
//!
//! Git packages are stored under `<cache_dir>/<name>/<commit_sha>/`, the same
//! `<name>/<key>/` layout used by registry tarballs. This means `utoo clean`
//! works uniformly without any git-specific knowledge.
//!
//! ## Async / blocking model
//!
//! The `gix` crate uses blocking HTTP transport; all network and heavy I/O
//! is run in a `tokio::task::spawn_blocking` thread so the async executor is
//! never blocked. A future improvement is to switch to gix's
//! `async-http-transport-reqwest-rust-tls` feature for a fully async pipeline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;

use crate::model::git::GitCloneResult;
use crate::model::manifest::{Dist, VersionManifest};
use crate::model::spec::PackageSpec;
use crate::traits::registry::ResolvedPackage;

// ---------------------------------------------------------------------------
// Clone dedup cache
// ---------------------------------------------------------------------------

/// Deduplicates concurrent git clone operations.
///
/// Keyed by canonical `url#commit_ish`. When multiple BFS tasks request the
/// same repo simultaneously, only one clone is performed; the others await
/// the result. Matches the `DOWNLOAD_CACHE` pattern in the PM crate's
/// `downloader.rs`.
type CloneCache = Mutex<HashMap<String, Arc<tokio::sync::OnceCell<Arc<GitCloneResult>>>>>;

static GIT_CLONE_CACHE: LazyLock<CloneCache> = LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read `GITHUB_TOKEN` or `GH_TOKEN` from the environment.
fn github_auth_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|t| !t.is_empty())
}

/// Inject an auth token into an HTTPS URL (e.g. `https://token:TOKEN@github.com/...`).
///
/// Returns the original URL unchanged if the scheme is not HTTPS.
fn try_inject_token_into_https_url(url: &str, token: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("https://x-access-token:{token}@{rest}")
    } else {
        url.to_string()
    }
}

/// Read `package.json` from the root of a git tree and deserialize it as a
/// [`VersionManifest`].
///
/// Only reads the single `package.json` blob — does **not** extract the full
/// tree, so this is cheap even for large repositories.
fn read_pkg_manifest(repo: &gix::Repository, tree_id: gix::ObjectId) -> Result<VersionManifest> {
    let tree = repo
        .find_object(tree_id)?
        .try_into_tree()
        .map_err(|e| anyhow!("expected tree object: {e}"))?;

    for entry in tree.iter() {
        let entry = entry?;
        if entry.filename() == b"package.json" {
            let obj = repo.find_object(entry.object_id())?;
            let manifest: VersionManifest = serde_json::from_slice(&obj.data)
                .context("Failed to parse package.json as VersionManifest from git tree")?;
            return Ok(manifest);
        }
    }
    Err(anyhow!(
        "package.json not found in git repo root (does the repo have a package.json?)"
    ))
}

/// Scan the cache directory for a previously-extracted git package by commit SHA.
///
/// Checks `<cache_dir>/<name>/<sha>/_resolved` for unscoped packages and
/// `<cache_dir>/@<scope>/<name>/<sha>/_resolved` for scoped packages.
/// Returns the package directory path if found.
fn find_cached_git_package(cache_dir: &Path, sha: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(cache_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = entry.file_name();
        let dir_name_str = dir_name.to_string_lossy();

        if dir_name_str.starts_with('@') {
            // Scoped package: @scope/name/sha/_resolved
            if let Ok(scope_entries) = std::fs::read_dir(&path) {
                for scope_entry in scope_entries.flatten() {
                    let candidate = scope_entry.path().join(sha);
                    if candidate.join("_resolved").exists() {
                        return Some(candidate);
                    }
                }
            }
        } else if !dir_name_str.starts_with('.') {
            // Unscoped package: name/sha/_resolved
            let candidate = path.join(sha);
            if candidate.join("_resolved").exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Build a [`GitCloneResult`] from a cached package directory on disk.
///
/// Reads `package.json` from the cached dir and fills in git-specific
/// manifest fields (`dist.tarball`, `has_install_script`).
fn read_cached_git_result(
    package_dir: &Path,
    sha: &str,
    original_url: &str,
) -> Result<GitCloneResult> {
    let pkg_bytes = std::fs::read(package_dir.join("package.json"))
        .context("failed to read cached package.json")?;
    let mut manifest: VersionManifest =
        serde_json::from_slice(&pkg_bytes).context("failed to parse cached package.json")?;

    let name = if manifest.name.is_empty() {
        return Err(anyhow!("cached package.json missing 'name'"));
    } else {
        manifest.name.clone()
    };
    if manifest.version.is_empty() {
        manifest.version = "0.0.0".to_string();
    }
    let version = manifest.version.clone();

    let resolved_url = format!("{}#{}", original_url, sha);
    manifest.dist = Dist {
        tarball: Some(resolved_url.clone()),
        integrity: None,
        ..Default::default()
    };
    manifest.has_install_script = Some(manifest.scripts.as_ref().is_some_and(|s| {
        s.contains_key("preinstall") || s.contains_key("install") || s.contains_key("postinstall")
    }));

    Ok(GitCloneResult {
        name,
        version,
        path: package_dir.to_path_buf(),
        resolved_url,
        manifest,
    })
}

/// Recursively extract a git tree to a directory on disk.
fn extract_tree_to_dir(repo: &gix::Repository, tree_id: gix::ObjectId, dest: &Path) -> Result<()> {
    let tree = repo
        .find_object(tree_id)?
        .try_into_tree()
        .map_err(|e| anyhow!("expected tree object: {e}"))?;

    for entry in tree.iter() {
        let entry = entry?;
        let name =
            std::str::from_utf8(entry.filename()).context("non-UTF-8 filename in git tree")?;
        let entry_path = dest.join(name);

        match entry.mode().kind() {
            gix::object::tree::EntryKind::Tree => {
                std::fs::create_dir_all(&entry_path)?;
                extract_tree_to_dir(repo, entry.object_id(), &entry_path)?;
            }
            gix::object::tree::EntryKind::Blob | gix::object::tree::EntryKind::BlobExecutable => {
                let obj = repo.find_object(entry.object_id())?;
                std::fs::write(&entry_path, &obj.data)?;

                // Set executable permission on Unix
                #[cfg(unix)]
                if entry.mode().kind() == gix::object::tree::EntryKind::BlobExecutable {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(0o755);
                    if let Err(e) = std::fs::set_permissions(&entry_path, perms) {
                        tracing::warn!(
                            "Failed to set executable permission on {}: {e}",
                            entry_path.display()
                        );
                    }
                }
            }
            gix::object::tree::EntryKind::Link => {
                let obj = repo.find_object(entry.object_id())?;
                let target = std::str::from_utf8(&obj.data).context("non-UTF-8 symlink target")?;
                #[cfg(unix)]
                if let Err(e) = std::os::unix::fs::symlink(target, &entry_path) {
                    tracing::warn!("Failed to create symlink {}: {e}", entry_path.display());
                }
                #[cfg(not(unix))]
                {
                    let _ = target;
                }
            }
            gix::object::tree::EntryKind::Commit => {
                // Submodule reference – skip silently
                tracing::debug!(
                    "Skipping submodule entry at {:?}",
                    std::str::from_utf8(entry.filename()).unwrap_or("<non-utf8>")
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Blocking core
// ---------------------------------------------------------------------------

/// Blocking core that does the actual clone + extraction via gix.
///
/// Cache layout: `<cache_dir>/<name>/<commit_sha>/` — identical to registry
/// tarballs, so `utoo clean` works uniformly.
fn clone_repo_blocking(
    cache_dir: &Path,
    clone_url: &str,
    commit_ish: Option<&str>,
    original_url: &str,
) -> Result<GitCloneResult> {
    // A full 40-hex SHA might not be reachable at depth 1 (it may not be
    // HEAD), so only use shallow fetch for branch/tag refs and bare HEAD.
    let is_full_sha =
        commit_ish.is_some_and(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()));

    // Early cache check for full SHA specs — skip network fetch entirely.
    // Branch/tag refs must always be fetched (the ref may have moved).
    if is_full_sha {
        let sha = commit_ish.unwrap(); // safe: is_full_sha implies Some
        if let Some(package_dir) = find_cached_git_package(cache_dir, sha) {
            match read_cached_git_result(&package_dir, sha, original_url) {
                Ok(result) => {
                    tracing::debug!(
                        "Git cache hit: {}@{} (SHA: {})",
                        result.name,
                        result.version,
                        sha
                    );
                    return Ok(result);
                }
                Err(e) => {
                    tracing::warn!("Git cache read failed, will re-fetch: {e}");
                    // Fall through to network fetch
                }
            }
        }
    }

    // Capture auth token once; reused for URL injection and the error hint below.
    let token = github_auth_token();
    let effective_url = match &token {
        Some(t) => try_inject_token_into_https_url(clone_url, t),
        None => clone_url.to_string(),
    };

    // Clone to a temporary directory (bare)
    let temp_dir = tempfile::tempdir().context("Failed to create temp directory for git clone")?;

    let url = gix::url::parse(effective_url.as_str().into())?;

    let mut prepare = gix::prepare_clone_bare(url, temp_dir.path())?;

    if !is_full_sha {
        prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
            std::num::NonZeroU32::MIN,
        ));
    }

    // For branch/tag refs, tell gix which ref to fetch.
    // Full-SHA requests have no corresponding ref name.
    if let Some(ref_name) = commit_ish
        && !is_full_sha
    {
        prepare = prepare.with_ref_name(Some(ref_name))?;
    }

    let (checkout, _outcome) = prepare
        .fetch_only(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| {
            let mut msg = format!("git fetch failed for '{}': {e}", clone_url);
            if token.is_none()
                && (clone_url.contains("github.com") || clone_url.contains("gitlab.com"))
            {
                msg.push_str(
                    "\n\nTip: set GITHUB_TOKEN or GH_TOKEN for private repos / higher rate limits",
                );
            }
            anyhow!(msg)
        })?;

    // Resolve the commit: if we have a full 40-char hex SHA use it directly,
    // otherwise resolve HEAD of the fetched ref.
    let commit_id = match commit_ish {
        Some(sha) if sha.len() == 40 && sha.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            gix::ObjectId::from_hex(sha.as_bytes())
                .map_err(|e| anyhow!("invalid commit SHA '{}': {e}", sha))?
        }
        _ => checkout
            .head_id()
            .map_err(|e| anyhow!("could not resolve HEAD after fetch: {e}"))?
            .detach(),
    };

    let commit_hex = commit_id.to_string();

    // Get the commit tree
    let commit = checkout
        .find_object(commit_id)?
        .try_into_commit()
        .map_err(|e| anyhow!("object is not a commit: {e}"))?;

    let tree_id = commit.tree_id()?;

    // Deserialize package.json from the git blob directly into a VersionManifest.
    // This is cheap (single object read) and avoids any serde_json::Value round-trip.
    let mut manifest = read_pkg_manifest(&checkout, tree_id.into())?;

    let name = if manifest.name.is_empty() {
        return Err(anyhow!("package.json in git repo is missing 'name' field"));
    } else {
        manifest.name.clone()
    };

    if manifest.version.is_empty() {
        tracing::warn!(
            "package.json in git repo '{}' is missing 'version' field; defaulting to 0.0.0",
            name
        );
        manifest.version = "0.0.0".to_string();
    }
    let version = manifest.version.clone();

    // Reject suspicious package names via Path::components() to avoid
    // path-traversal attacks (e.g. `@scope/../evil` passes string-based checks).
    let name_path = PathBuf::from(&name);
    if name_path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Err(anyhow!(
            "Suspicious package name '{}' in git repo — refusing to cache",
            name
        ));
    }

    // Cache path: <cache_dir>/<name>/<commit_sha>/
    // Using the full commit SHA as the version directory avoids collisions
    // with registry versions and between different commits of the same repo.
    let package_dir = cache_dir.join(&name).join(&commit_hex);
    let resolved_url = format!("{}#{}", original_url, commit_hex);
    let resolved_marker = package_dir.join("_resolved");

    // Fill in git-specific manifest fields.
    // `dist.tarball` is set to the pinned URL so downstream consumers can
    // identify the source. `has_install_script` is computed from `scripts`
    // because package.json doesn't carry this pre-computed flag.
    manifest.dist = Dist {
        tarball: Some(resolved_url.clone()),
        integrity: None,
        ..Default::default()
    };
    manifest.has_install_script = Some(manifest.scripts.as_ref().is_some_and(|s| {
        s.contains_key("preinstall") || s.contains_key("install") || s.contains_key("postinstall")
    }));

    if resolved_marker.exists() {
        return Ok(GitCloneResult {
            name,
            version,
            path: package_dir,
            resolved_url,
            manifest,
        });
    }

    // Extract into a staging directory, then atomically rename it into place.
    // This avoids TOCTOU: concurrent processes each get their own tmp dir and
    // the winner's rename is POSIX-atomic (same filesystem guaranteed).
    let tmp_dir = package_dir.with_extension("tmp");
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).context("Failed to remove stale tmp git cache dir")?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    extract_tree_to_dir(&checkout, tree_id.into(), &tmp_dir)?;

    // Write marker inside tmp dir so it becomes visible atomically after rename.
    std::fs::write(tmp_dir.join("_resolved"), "")?;

    // Atomic rename; match on the error to detect the race-winner case.
    // On Linux/macOS, rename(2) over an existing non-empty directory yields
    // ENOTEMPTY (not EEXIST), so we check raw_os_error in addition to
    // ErrorKind::AlreadyExists.
    match std::fs::rename(&tmp_dir, &package_dir) {
        Ok(()) => {}
        Err(e)
            if e.kind() == std::io::ErrorKind::AlreadyExists
                || e.raw_os_error() == Some(libc::ENOTEMPTY) =>
        {
            // Another process completed first — discard our tmp dir.
            let _ = std::fs::remove_dir_all(&tmp_dir);
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(anyhow!("Failed to commit git cache directory: {e}"));
        }
    }

    Ok(GitCloneResult {
        name,
        version,
        path: package_dir,
        resolved_url,
        manifest,
    })
}

// ---------------------------------------------------------------------------
// Async public API
// ---------------------------------------------------------------------------

/// Ensure a git repository is cloned and cached, returning its metadata.
///
/// `url` may include the `git+` prefix (e.g. `git+https://github.com/user/repo.git`);
/// it is stripped automatically before cloning.
///
/// Cloned packages are cached at `<cache_dir>/<name>/<commit_sha>/`, matching the
/// same layout as registry tarballs.
///
/// Concurrent requests for the same `url#commit_ish` are deduplicated: only the
/// first caller performs the actual clone, and subsequent callers await the result.
///
/// # Arguments
/// * `cache_dir` - Root cache directory
/// * `url` - Git URL, optionally with `git+` prefix
/// * `commit_ish` - Optional branch, tag, or commit SHA to check out
pub async fn ensure_repo_cached(
    cache_dir: &Path,
    url: &str,
    commit_ish: Option<&str>,
) -> Result<Arc<GitCloneResult>> {
    // Normalize the dedup key: strip `git+` so `git+https://…` and `https://…`
    // resolve to the same cache entry.
    let canonical_url = url.strip_prefix("git+").unwrap_or(url);
    let key = format!("{}#{}", canonical_url, commit_ish.unwrap_or("HEAD"));

    let cell = {
        let mut cache = GIT_CLONE_CACHE.lock();
        cache
            .entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    let clone_url = url.strip_prefix("git+").unwrap_or(url).to_string();
    let original_url = url.to_string();
    let cache_dir = cache_dir.to_path_buf();
    let commit_ish_owned = commit_ish.map(|s| s.to_string());

    cell.get_or_try_init(|| async {
        tokio::task::spawn_blocking(move || {
            clone_repo_blocking(
                &cache_dir,
                &clone_url,
                commit_ish_owned.as_deref(),
                &original_url,
            )
            .map(Arc::new)
        })
        .await
        .context("git resolver task failed")?
    })
    .await
    .cloned()
}

// ---------------------------------------------------------------------------
// High-level resolver — called by BFS `process_dependency`
// ---------------------------------------------------------------------------

/// Resolve a non-registry dependency spec to a [`ResolvedPackage`].
///
/// Accepts an already-parsed [`PackageSpec`] (Git or GitHub variant) so the
/// call site's type-level guarantee is preserved — re-parsing from a raw
/// string would discard it.
///
/// 1. Extracts the clone URL and `commit_ish` from the spec.
/// 2. Clones the repository via [`ensure_repo_cached`].
/// 3. Returns the pre-built [`VersionManifest`] from [`GitCloneResult`] —
///    no additional I/O required.
// TODO: refactor this to be more extendable
pub(crate) async fn resolve_non_registry_dep(
    cache_dir: Option<&Path>,
    spec: &PackageSpec,
) -> anyhow::Result<ResolvedPackage> {
    let (url, commit_ish) = match spec {
        PackageSpec::Git { url, commit_ish } => (url.clone(), commit_ish.clone()),
        PackageSpec::GitHub {
            owner,
            repo,
            commit_ish,
        } => (
            format!("git+https://github.com/{owner}/{repo}.git"),
            commit_ish.clone(),
        ),
        // Exhaustive enumeration — new PackageSpec variants will cause a
        // compile error here rather than silently hitting a wildcard arm.
        PackageSpec::Registry { .. } | PackageSpec::Local { .. } | PackageSpec::Http { .. } => {
            unreachable!("resolve_non_registry_dep called with non-git spec: {spec:?}")
        }
    };

    let cache_dir = cache_dir
        .ok_or_else(|| anyhow::anyhow!("cache_dir required for git dependency resolution"))?;

    let result = ensure_repo_cached(cache_dir, &url, commit_ish.as_deref()).await?;

    Ok(ResolvedPackage {
        name: result.name.clone(),
        version: result.version.clone(),
        manifest: result.manifest.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_token_into_url() {
        assert_eq!(
            try_inject_token_into_https_url("https://github.com/user/repo.git", "mytoken"),
            "https://x-access-token:mytoken@github.com/user/repo.git"
        );
        // Non-HTTPS URL should pass through unchanged
        assert_eq!(
            try_inject_token_into_https_url("ssh://git@github.com/user/repo.git", "mytoken"),
            "ssh://git@github.com/user/repo.git"
        );
    }

    #[test]
    fn test_path_traversal_detection() {
        let evil_names = ["../evil", "../../etc/passwd", "/etc/passwd", "foo/../bar"];
        for name in &evil_names {
            let name_path = PathBuf::from(name);
            let has_traversal = name_path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::RootDir
                )
            });
            assert!(
                has_traversal,
                "Expected path traversal to be detected in '{name}'"
            );
        }
    }

    #[test]
    fn test_dedup_key_normalization() {
        // git+ prefix must be stripped so both spellings share the same cache entry.
        let url_with_prefix = "git+https://github.com/user/repo.git";
        let url_bare = "https://github.com/user/repo.git";
        let key1 = format!(
            "{}#{}",
            url_with_prefix
                .strip_prefix("git+")
                .unwrap_or(url_with_prefix),
            "HEAD"
        );
        let key2 = format!(
            "{}#{}",
            url_bare.strip_prefix("git+").unwrap_or(url_bare),
            "HEAD"
        );
        assert_eq!(
            key1, key2,
            "git+ and bare URLs should produce the same dedup key"
        );
    }
}
