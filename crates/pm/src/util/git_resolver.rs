//! Git package resolver.
//!
//! Thin wrappers around ruborist's git clone functionality,
//! injecting the PM cache directory.

use std::sync::Arc;

use anyhow::Result;
pub use utoo_ruborist::git::{GitCloneCache, GitCloneResult, ensure_repo_cached};

use super::cache::get_cache_dir;

/// Create a new empty git clone cache for deduplicating concurrent clones.
pub fn new_git_clone_cache() -> GitCloneCache {
    Default::default()
}

/// Extract a reasonable package name from a git URL for cache-path purposes.
fn name_from_url(url: &str) -> &str {
    let clean = url.strip_prefix("git+").unwrap_or(url);
    let segment = clean.rsplit('/').next().unwrap_or("unknown");
    segment.strip_suffix(".git").unwrap_or(segment)
}

/// Resolve a git package spec by cloning the repo, checking out the ref,
/// reading package.json, and caching the result.
pub async fn resolve_git_spec(
    url: &str,
    commit_ish: Option<&str>,
    dep_name: Option<&str>,
    clone_cache: &GitCloneCache,
) -> Result<Arc<GitCloneResult>> {
    let cache_dir = get_cache_dir();
    let name = dep_name.unwrap_or_else(|| name_from_url(url));
    ensure_repo_cached(&cache_dir, url, commit_ish, name, clone_cache).await
}

/// Convert a `github:owner/repo` shorthand to a git+ URL and resolve.
pub async fn resolve_github_spec(
    owner: &str,
    repo: &str,
    commit_ish: Option<&str>,
    clone_cache: &GitCloneCache,
) -> Result<Arc<GitCloneResult>> {
    let url = format!("git+https://github.com/{}/{}.git", owner, repo);
    resolve_git_spec(&url, commit_ish, Some(repo), clone_cache).await
}
