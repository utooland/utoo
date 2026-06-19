//! Git clone backend powered by the system `git` binary.
//!
//! Git packages are stored under `<cache_dir>/<name>/<commit_sha>/`, the same
//! `<name>/<key>/` layout used by registry tarballs, so `utoo clean` works
//! uniformly without any git-specific knowledge.
//!
//! Rather than bundling a Rust git implementation, we shell out to the user's
//! `git` (the same approach npm/pnpm/yarn take) — it keeps the binary small,
//! reuses the user's existing git auth/config, and avoids carrying a second
//! HTTP+TLS stack. `git` is required only for `git:`/`github:` dependencies; if
//! it is missing, a clear error names it. All clone/checkout work runs in a
//! `tokio::task::spawn_blocking` thread so the async executor is never blocked.

use std::path::Path;
use std::process::Command;
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

/// Run `git` with the given args (optionally in `cwd`), returning trimmed stdout.
///
/// `GIT_TERMINAL_PROMPT=0` keeps a missing-credentials clone from hanging on an
/// interactive prompt. A missing `git` binary is reported as a clear, named
/// error rather than a generic spawn failure.
fn run_git(cwd: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");

    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow!("`git` command not found in PATH — it is required to resolve git dependencies")
        } else {
            anyhow!("failed to run git: {e}")
        }
    })?;

    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

/// Copy a checked-out working tree into `dest`, skipping `.git` and preserving
/// executable bits (via `std::fs::copy`) and symlinks.
///
/// `root_dest` is the top-level destination; symlinks whose target would escape
/// it are skipped to prevent path-traversal attacks (matching the previous
/// tree-extraction behaviour).
fn copy_worktree(src: &Path, dest: &Path, root_dest: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        // Skip git's own metadata (only present at the repo root, but a
        // submodule gitlink could leave a `.git` file deeper — skip defensively).
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&name);
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            let target = std::fs::read_link(&from)?;
            // Validate that the symlink target does not escape the destination
            // root to prevent path-traversal attacks.
            let resolved = to.parent().unwrap_or(dest).join(&target);
            if !resolved.starts_with(root_dest) {
                tracing::debug!(
                    "Skipping symlink {} -> {} (escapes extraction dir)",
                    to.display(),
                    target.display()
                );
                continue;
            }
            #[cfg(unix)]
            if let Err(e) = std::os::unix::fs::symlink(&target, &to) {
                tracing::debug!("Failed to create symlink {}: {e}", to.display());
            }
            #[cfg(not(unix))]
            {
                let _ = target;
            }
        } else if file_type.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_worktree(&from, &to, root_dest)?;
        } else {
            // `std::fs::copy` preserves the permission bits (incl. the exec bit)
            // on Unix, so an executable in the repo stays executable.
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Clone `clone_url` (optionally at `commit_ish`) into `dest`, shelling out to
/// `git`. Prefers a shallow clone for branch/tag/HEAD; a full SHA (or a ref a
/// shallow `--branch` can't name) falls back to a full clone + checkout.
fn git_clone_into(
    dest: &Path,
    clone_url: &str,
    commit_ish: Option<&str>,
    is_full_sha: bool,
) -> Result<()> {
    let dest_str = dest
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 clone path"))?;

    // Shallow fast path: a named branch/tag, or the default HEAD.
    if !is_full_sha {
        let shallow = match commit_ish {
            Some(ref_name) => run_git(
                None,
                &[
                    "clone", "--quiet", "--depth", "1", "--branch", ref_name, clone_url, dest_str,
                ],
            ),
            None => run_git(
                None,
                &["clone", "--quiet", "--depth", "1", clone_url, dest_str],
            ),
        };
        match shallow {
            Ok(_) => return Ok(()),
            // `--branch` only names branches/tags; a short SHA (or other ref)
            // falls through to a full clone + checkout below.
            Err(_) => {
                let _ = std::fs::remove_dir_all(dest);
            }
        }
    }

    // Full clone, then check out the requested commit-ish (SHA, short SHA, …).
    run_git(None, &["clone", "--quiet", clone_url, dest_str])?;
    if let Some(target) = commit_ish {
        run_git(Some(dest), &["checkout", "--quiet", target])?;
    }
    Ok(())
}

/// Blocking core that does the actual clone + checkout via the `git` binary.
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

    // A full 40-hex SHA can't be named by a shallow `--branch`, so it takes the
    // full-clone path.
    let is_full_sha =
        commit_ish.is_some_and(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()));

    // Early cache check for full SHA specs — skip the clone entirely. The
    // package name is known at the call site so we check the exact path rather
    // than scanning directories.
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

    // Clone into a temporary working tree.
    let temp_dir = tempfile::tempdir().context("Failed to create temp directory for git clone")?;
    let checkout = temp_dir.path().join("repo");

    git_clone_into(&checkout, &effective_url, commit_ish, is_full_sha).map_err(|e| {
        let mut msg = format!("git clone failed for '{clone_url}': {e}");
        if token.is_none() && (clone_url.contains("github.com") || clone_url.contains("gitlab.com"))
        {
            msg.push_str(
                "\n\nTip: set GITHUB_TOKEN or GH_TOKEN for private repos / higher rate limits",
            );
        }
        anyhow!(msg)
    })?;

    // The checked-out commit SHA pins the cache directory.
    let commit_hex = run_git(Some(&checkout), &["rev-parse", "HEAD"])
        .context("could not resolve HEAD after clone")?;

    // Read package.json straight from the working tree.
    let pkg_bytes = std::fs::read(checkout.join("package.json"))
        .context("package.json not found in git repo root (does the repo have a package.json?)")?;
    let mut manifest: CoreVersionManifest =
        serde_json::from_slice(&pkg_bytes).context("Failed to parse package.json from git repo")?;

    // Cache path: <cache_dir>/<name>/<commit_sha>/. Using the full commit SHA as
    // the version directory avoids collisions with registry versions and between
    // different commits of the same repo.
    let pinned_url = format!("{}#{}", resolved_url, commit_hex);
    finalize_non_registry_manifest(&mut manifest, pinned_url.clone())?;

    let package_dir = cache_dir.join(&manifest.name).join(&commit_hex);
    if package_dir.join("_resolved").exists() {
        return Ok(GitCloneResult::new(package_dir, pinned_url, manifest));
    }

    commit_cache_dir_atomic(&package_dir, |stage| copy_worktree(&checkout, stage, stage))?;

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
