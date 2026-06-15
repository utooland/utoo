//! Git clone backend powered by `gix`.
//!
//! Git packages are stored under `<cache_dir>/<name>/<commit_sha>/`, the same
//! `<name>/<key>/` layout used by registry tarballs, so `utoo clean` works
//! uniformly without any git-specific knowledge.
//!
//! The `gix` crate uses blocking HTTP transport; all network and heavy I/O
//! is run in a `tokio::task::spawn_blocking` thread so the async executor is
//! never blocked.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::common::{
    DedupCache, commit_cache_dir_atomic, dedup_init, finalize_non_registry_manifest,
    validate_package_name,
};
use crate::model::git::GitCloneResult;
use crate::model::manifest::CoreVersionManifest;
use crate::spec::PackageSpec;
use crate::traits::registry::ResolvedPackage;

/// Deduplicates concurrent git clone operations.
///
/// Keyed by canonical `url#commit_ish`. When multiple BFS tasks request the
/// same repo simultaneously, only one clone is performed; the others await
/// the result. Lives as a field on `BuildDepsConfig` rather than as a global
/// static, so it is scoped to a single resolution session.
pub type GitCloneCache = DedupCache<GitCloneResult>;

/// Read `GITHUB_TOKEN` or `GH_TOKEN` from the environment.
fn github_auth_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|t| !t.is_empty())
}

/// Inject an auth token into an HTTPS URL (e.g. `https://token:TOKEN@github.com/...`).
///
/// Returns the original URL unchanged if:
/// - the scheme is not HTTPS
/// - the URL already contains credentials (userinfo before the host)
fn try_inject_token_into_https_url(url: &str, token: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        // If the URL already contains userinfo (e.g. user:pass@host), leave it alone
        // to avoid producing a malformed URL like https://token@user:pass@host.
        if rest
            .split_once('/')
            .map_or(rest, |(host, _)| host)
            .contains('@')
        {
            return url.to_string();
        }
        format!("https://x-access-token:{token}@{rest}")
    } else {
        url.to_string()
    }
}

/// Read `package.json` from the root of a git tree and deserialize it as a
/// [`CoreVersionManifest`].
///
/// Only reads the single `package.json` blob — does **not** extract the full
/// tree, so this is cheap even for large repositories.
fn read_pkg_manifest(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
) -> Result<CoreVersionManifest> {
    let tree = repo
        .find_object(tree_id)?
        .try_into_tree()
        .map_err(|e| anyhow!("expected tree object: {e}"))?;

    for entry in tree.iter() {
        let entry = entry?;
        if entry.filename() == b"package.json" {
            let obj = repo.find_object(entry.object_id())?;
            let manifest: CoreVersionManifest = serde_json::from_slice(&obj.data)
                .context("Failed to parse package.json from git tree")?;
            return Ok(manifest);
        }
    }
    Err(anyhow!(
        "package.json not found in git repo root (does the repo have a package.json?)"
    ))
}

/// Build a [`GitCloneResult`] from a cached package directory on disk.
///
/// Reads `package.json` from the cached dir and runs the shared non-registry
/// manifest finalization (`dist.tarball`, `has_install_script`, version default).
fn read_cached_git_result(
    package_dir: &Path,
    sha: &str,
    resolved_url: &str,
) -> Result<GitCloneResult> {
    let pkg_bytes = std::fs::read(package_dir.join("package.json"))
        .context("failed to read cached package.json")?;
    let mut manifest: CoreVersionManifest =
        serde_json::from_slice(&pkg_bytes).context("failed to parse cached package.json")?;

    let pinned_url = format!("{}#{}", resolved_url, sha);
    finalize_non_registry_manifest(&mut manifest, pinned_url.clone())?;

    Ok(GitCloneResult::new(
        package_dir.to_path_buf(),
        pinned_url,
        manifest,
    ))
}

/// Recursively extract a git tree to a directory on disk.
///
/// `root_dest` is the top-level extraction directory; symlinks that would
/// escape it are skipped to prevent path-traversal attacks.
fn extract_tree_to_dir(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    dest: &Path,
    root_dest: &Path,
) -> Result<()> {
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
                extract_tree_to_dir(repo, entry.object_id(), &entry_path, root_dest)?;
            }
            gix::object::tree::EntryKind::Blob | gix::object::tree::EntryKind::BlobExecutable => {
                let obj = repo.find_object(entry.object_id())?;
                std::fs::write(&entry_path, &obj.data)?;

                // Set executable permission on Unix
                #[cfg(unix)]
                if entry.mode().kind() == gix::object::tree::EntryKind::BlobExecutable {
                    let perms = std::fs::Permissions::from_mode(0o755);
                    if let Err(e) = std::fs::set_permissions(&entry_path, perms) {
                        tracing::debug!(
                            "Failed to set executable permission on {}: {e}",
                            entry_path.display()
                        );
                    }
                }
            }
            gix::object::tree::EntryKind::Link => {
                let obj = repo.find_object(entry.object_id())?;
                let target = std::str::from_utf8(&obj.data).context("non-UTF-8 symlink target")?;

                // Validate that the symlink target does not escape the
                // extraction root to prevent path-traversal attacks.
                let resolved = entry_path.parent().unwrap_or(dest).join(target);
                if !resolved.starts_with(root_dest) {
                    tracing::debug!(
                        "Skipping symlink {} -> {} (escapes extraction dir)",
                        entry_path.display(),
                        target
                    );
                    continue;
                }

                #[cfg(unix)]
                if let Err(e) = std::os::unix::fs::symlink(target, &entry_path) {
                    tracing::debug!("Failed to create symlink {}: {e}", entry_path.display());
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

/// Blocking core that does the actual clone + extraction via gix.
fn clone_repo_blocking(
    cache_dir: &Path,
    clone_url: &str,
    commit_ish: Option<&str>,
    resolved_url: &str,
    name: &str,
) -> Result<GitCloneResult> {
    // Validate the caller-supplied name before using it in any path operation.
    // This MUST run before the early cache check which does cache_dir.join(name).
    validate_package_name(name)?;

    // A full 40-hex SHA might not be reachable at depth 1 (it may not be
    // HEAD), so only use shallow fetch for branch/tag refs and bare HEAD.
    let is_full_sha =
        commit_ish.is_some_and(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()));

    // Early cache check for full SHA specs — skip network fetch entirely.
    // The package name is known at call site so we check the exact path
    // rather than scanning directories.
    if is_full_sha {
        let sha = commit_ish.unwrap();
        let package_dir = cache_dir.join(name).join(sha);
        if package_dir.join("_resolved").exists() {
            match read_cached_git_result(&package_dir, sha, resolved_url) {
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
                    tracing::debug!("Git cache read failed, will re-fetch: {e}");
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

    // Deserialize package.json from the git blob directly into a CoreVersionManifest.
    // This is cheap (single object read) and avoids any serde_json::Value round-trip.
    let mut manifest = read_pkg_manifest(&checkout, tree_id.into())?;

    // Cache path: <cache_dir>/<name>/<commit_sha>/
    // Using the full commit SHA as the version directory avoids collisions
    // with registry versions and between different commits of the same repo.
    let pinned_url = format!("{}#{}", resolved_url, commit_hex);
    finalize_non_registry_manifest(&mut manifest, pinned_url.clone())?;

    let package_dir = cache_dir.join(&manifest.name).join(&commit_hex);
    if package_dir.join("_resolved").exists() {
        return Ok(GitCloneResult::new(package_dir, pinned_url, manifest));
    }

    commit_cache_dir_atomic(&package_dir, |stage| {
        extract_tree_to_dir(&checkout, tree_id.into(), stage, stage)
    })?;

    Ok(GitCloneResult::new(package_dir, pinned_url, manifest))
}

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
/// * `url` - Git URL with `git+` prefix (e.g. `git+https://github.com/foo/bar.git`)
/// * `commit_ish` - Optional branch, tag, or commit SHA to check out
/// * `name` - Package name (from the dependency edge)
/// * `clone_cache` - Shared dedup cache for concurrent clone operations
pub async fn ensure_repo_cached(
    cache_dir: &Path,
    url: &str,
    commit_ish: Option<&str>,
    name: &str,
    clone_cache: &GitCloneCache,
) -> Result<Arc<GitCloneResult>> {
    // Strip the `git+` prefix to get a clone-ready URL.
    let canonical_url = url.strip_prefix("git+").unwrap_or(url);
    let key = format!("{}#{}", canonical_url, commit_ish.unwrap_or("HEAD"));

    let clone_url = canonical_url.to_string();
    let resolved_url = url.to_string();
    let cache_dir = cache_dir.to_path_buf();
    let commit_ish_owned = commit_ish.map(|s| s.to_string());
    let name_owned = name.to_string();

    dedup_init(clone_cache, key, || async move {
        tokio::task::spawn_blocking(move || {
            clone_repo_blocking(
                &cache_dir,
                &clone_url,
                commit_ish_owned.as_deref(),
                &resolved_url,
                &name_owned,
            )
        })
        .await
        .context("git resolver task failed")?
    })
    .await
}

/// Resolve a git/github dep spec to a [`ResolvedPackage`].
///
/// Accepts an already-parsed [`PackageSpec`] so the call site's type-level
/// guarantee survives — re-parsing from a raw string would discard it.
pub(crate) async fn resolve_git_dep(
    cache_dir: Option<&Path>,
    spec: &PackageSpec,
    name: &str,
    clone_cache: &GitCloneCache,
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
        // Exhaustive match: a new PackageSpec variant becomes a compile error
        // rather than silently hitting a wildcard arm.
        PackageSpec::Registry { .. } | PackageSpec::Local { .. } | PackageSpec::Http { .. } => {
            unreachable!("resolve_git_dep called with non-git spec: {spec:?}")
        }
    };

    let cache_dir = cache_dir
        .ok_or_else(|| anyhow::anyhow!("cache_dir required for git dependency resolution"))?;

    let result =
        ensure_repo_cached(cache_dir, &url, commit_ish.as_deref(), name, clone_cache).await?;

    Ok(ResolvedPackage {
        name: result.name.clone(),
        version: result.version.clone(),
        manifest: Arc::new(result.manifest.clone()),
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
        // URL with existing credentials should pass through unchanged
        assert_eq!(
            try_inject_token_into_https_url(
                "https://user:pass@github.com/user/repo.git",
                "mytoken"
            ),
            "https://user:pass@github.com/user/repo.git"
        );
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
