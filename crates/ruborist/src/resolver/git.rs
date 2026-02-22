//! Git clone backend powered by `gix`.
//!
//! This module is only compiled when the `native-git` Cargo feature is enabled.
//! It provides the low-level clone + cache logic that
//! [`crate::traits::git::resolve_non_registry_dep`] calls during BFS
//! resolution.
//!
//! ## Cache layout
//!
//! Git packages are stored under `<cache_dir>/<name>/<version>/`, the same
//! layout used by registry tarballs.  This means `utoo clean` works uniformly
//! without any git-specific knowledge.

use anyhow::{Context, Result, anyhow};
use std::path::Path;

use crate::traits::git::GitCloneResult;

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
fn inject_token_into_url(url: &str, token: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("https://x-access-token:{token}@{rest}")
    } else {
        url.to_string()
    }
}

/// Read `name` and `version` from `package.json` at the root of a git tree.
///
/// Only reads the single `package.json` blob — does **not** extract the full
/// tree, so this is cheap even for large repositories.
fn read_pkg_name_version(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
) -> Result<(String, String)> {
    let tree = repo
        .find_object(tree_id)?
        .try_into_tree()
        .map_err(|e| anyhow!("expected tree object: {e}"))?;

    for entry in tree.iter() {
        let entry = entry?;
        if entry.filename() == b"package.json" {
            let obj = repo.find_object(entry.object_id())?;
            let pkg: serde_json::Value = serde_json::from_slice(&obj.data)
                .context("Failed to parse package.json from git tree")?;
            let name = pkg
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("package.json in git repo is missing 'name' field"))?
                .to_string();
            let version = pkg
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string();
            return Ok((name, version));
        }
    }
    Err(anyhow!(
        "package.json not found in git repo root (does the repo have a package.json?)"
    ))
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
                    std::fs::set_permissions(&entry_path, perms).ok();
                }
            }
            gix::object::tree::EntryKind::Link => {
                let obj = repo.find_object(entry.object_id())?;
                let target = std::str::from_utf8(&obj.data).context("non-UTF-8 symlink target")?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &entry_path).ok();
                #[cfg(not(unix))]
                {
                    let _ = target;
                }
            }
            gix::object::tree::EntryKind::Commit => {
                // Submodule reference – skip
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
/// Cache layout: `<cache_dir>/<name>/<version>/` — identical to registry
/// tarballs, so `utoo clean` works uniformly.
fn clone_repo_blocking(
    cache_dir: &Path,
    clone_url: &str,
    commit_ish: Option<&str>,
    original_url: &str,
) -> Result<GitCloneResult> {
    // Optionally inject auth token for HTTPS URLs
    let effective_url = match github_auth_token() {
        Some(token) => inject_token_into_url(clone_url, &token),
        None => clone_url.to_string(),
    };

    // Clone to a temporary directory (bare)
    let temp_dir = tempfile::tempdir().context("Failed to create temp directory for git clone")?;

    let url = gix::url::parse(effective_url.as_str().into())?;

    let mut prepare = gix::prepare_clone_bare(url, temp_dir.path())?;

    // Configure shallow fetch (depth 1)
    prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
        std::num::NonZeroU32::new(1).unwrap(),
    ));

    // If commit_ish looks like a branch/tag, configure the ref to fetch
    let is_full_sha =
        commit_ish.is_some_and(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()));

    if let Some(ref_name) = commit_ish
        && !is_full_sha
    {
        // Try as branch/tag ref
        prepare = prepare.with_ref_name(Some(ref_name))?;
    }

    let (checkout, _outcome) = prepare
        .fetch_only(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| {
            let mut msg = format!("git fetch failed for '{}': {e}", clone_url);
            if github_auth_token().is_none()
                && (clone_url.contains("github.com") || clone_url.contains("gitlab.com"))
            {
                msg.push_str(
                    "\n\nTip: set GITHUB_TOKEN or GH_TOKEN for private repos / higher rate limits",
                );
            }
            anyhow!(msg)
        })?;

    // Resolve the commit
    let commit_id = if is_full_sha {
        let sha = commit_ish.unwrap();
        gix::ObjectId::from_hex(sha.as_bytes())
            .map_err(|e| anyhow!("invalid commit SHA '{}': {e}", sha))?
    } else {
        // HEAD of the fetched ref
        checkout
            .head_id()
            .map_err(|e| anyhow!("could not resolve HEAD after fetch: {e}"))?
            .detach()
    };

    let commit_hex = commit_id.to_string();

    // Get the commit tree
    let commit = checkout
        .find_object(commit_id)?
        .try_into_commit()
        .map_err(|e| anyhow!("object is not a commit: {e}"))?;

    let tree_id = commit.tree_id()?;

    // Read name/version from package.json blob (cheap — single object read)
    let (name, version) = read_pkg_name_version(&checkout, tree_id.into())?;

    // Cache path: <cache_dir>/<name>/<commit_sha>/
    // Using the full commit SHA as the version directory avoids collisions
    // with registry versions and between different commits of the same repo.
    // This also matches the layout registry tarballs use (<name>/<version>/),
    // so `utoo clean` works uniformly.
    let package_dir = cache_dir.join(&name).join(&commit_hex);
    let resolved_marker = package_dir.join("_resolved");

    if resolved_marker.exists() {
        return Ok(GitCloneResult {
            name,
            version,
            cache_path: package_dir,
            resolved_url: format!("{}#{}", original_url, commit_hex),
        });
    }

    // Extract full tree to cache
    if package_dir.exists() {
        std::fs::remove_dir_all(&package_dir).ok();
    }
    std::fs::create_dir_all(&package_dir)?;

    extract_tree_to_dir(&checkout, tree_id.into(), &package_dir)?;

    // Write _resolved marker
    std::fs::write(&resolved_marker, "")?;

    Ok(GitCloneResult {
        name,
        version,
        cache_path: package_dir,
        resolved_url: format!("{}#{}", original_url, commit_hex),
    })
}

// ---------------------------------------------------------------------------
// Async public API
// ---------------------------------------------------------------------------

/// Clone a git repository, cache the result, and return metadata.
///
/// `url` may include the `git+` prefix (e.g. `git+https://github.com/user/repo.git`);
/// it is stripped automatically before cloning.
///
/// Cloned packages are cached at `<cache_dir>/<name>/<version>/`, matching the
/// same layout as registry tarballs.
///
/// # Arguments
/// * `cache_dir` - Root cache directory
/// * `url` - Git URL, optionally with `git+` prefix
/// * `commit_ish` - Optional branch, tag, or commit SHA to check out
pub async fn clone_repo(
    cache_dir: &Path,
    url: &str,
    commit_ish: Option<&str>,
) -> Result<GitCloneResult> {
    let clone_url = url.strip_prefix("git+").unwrap_or(url).to_string();
    let original_url = url.to_string();
    let cache_dir = cache_dir.to_path_buf();
    let commit_ish_owned = commit_ish.map(|s| s.to_string());

    tokio::task::spawn_blocking(move || {
        clone_repo_blocking(
            &cache_dir,
            &clone_url,
            commit_ish_owned.as_deref(),
            &original_url,
        )
    })
    .await
    .context("git resolver task panicked")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_token_into_url() {
        assert_eq!(
            inject_token_into_url("https://github.com/user/repo.git", "mytoken"),
            "https://x-access-token:mytoken@github.com/user/repo.git"
        );
        // Non-HTTPS URL should pass through
        assert_eq!(
            inject_token_into_url("ssh://git@github.com/user/repo.git", "mytoken"),
            "ssh://git@github.com/user/repo.git"
        );
    }

    #[test]
    fn test_github_auth_token_missing() {
        // Just ensure the function doesn't panic
        let _ = github_auth_token();
    }
}
