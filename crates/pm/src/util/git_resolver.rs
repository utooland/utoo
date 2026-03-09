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
    let without_fragment = clean.split_once('#').map_or(clean, |(base, _)| base);
    let segment = without_fragment.rsplit('/').next().unwrap_or("unknown");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_from_url_basic() {
        assert_eq!(
            name_from_url("git+https://github.com/user/repo.git"),
            "repo"
        );
    }

    #[test]
    fn test_name_from_url_with_fragment() {
        assert_eq!(
            name_from_url("git+https://github.com/user/repo.git#v1.0.0"),
            "repo"
        );
        assert_eq!(
            name_from_url("git+https://github.com/user/repo.git#abc123"),
            "repo"
        );
    }

    #[test]
    fn test_name_from_url_no_git_suffix() {
        assert_eq!(
            name_from_url("git+https://github.com/user/my-lib"),
            "my-lib"
        );
        assert_eq!(
            name_from_url("git+https://github.com/user/my-lib#main"),
            "my-lib"
        );
    }

    #[test]
    fn test_name_from_url_bare_protocol() {
        assert_eq!(name_from_url("https://github.com/user/repo.git"), "repo");
        assert_eq!(name_from_url("git://github.com/user/repo.git"), "repo");
    }
}
