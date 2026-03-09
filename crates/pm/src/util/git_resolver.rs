//! Git package resolver.
//!
//! Thin wrappers around ruborist's git clone functionality,
//! injecting the PM cache directory.

use std::sync::Arc;

use anyhow::Result;
use once_cell::sync::Lazy;
pub use utoo_ruborist::git::{GitCloneCache, GitCloneResult, ensure_repo_cached};

use super::cache::get_cache_dir;

/// Global git clone cache shared across all resolve calls within a session.
/// Ensures the same repo is only cloned once even when multiple git deps
/// point to the same repository.
static GIT_CLONE_CACHE: Lazy<GitCloneCache> = Lazy::new(Default::default);

/// Extract a reasonable package name from a git URL for cache-path purposes.
fn name_from_url(url: &str) -> &str {
    let clean = url.strip_prefix("git+").unwrap_or(url);
    let without_fragment = clean.split_once('#').map_or(clean, |(base, _)| base);
    let trimmed = without_fragment.trim_end_matches('/');
    let segment = trimmed.rsplit('/').next().unwrap_or("unknown");
    segment.strip_suffix(".git").unwrap_or(segment)
}

/// Resolve a git package spec by cloning the repo, checking out the ref,
/// reading package.json, and caching the result.
///
/// Uses the global [`GIT_CLONE_CACHE`] for deduplication across calls.
pub async fn resolve_git_spec(
    url: &str,
    commit_ish: Option<&str>,
    dep_name: Option<&str>,
) -> Result<Arc<GitCloneResult>> {
    let cache_dir = get_cache_dir();
    let name = dep_name.unwrap_or_else(|| name_from_url(url));
    ensure_repo_cached(&cache_dir, url, commit_ish, name, &GIT_CLONE_CACHE).await
}

/// Convert a `github:owner/repo` shorthand to a git+ URL and resolve.
///
/// Uses the global [`GIT_CLONE_CACHE`] for deduplication across calls.
pub async fn resolve_github_spec(
    owner: &str,
    repo: &str,
    commit_ish: Option<&str>,
) -> Result<Arc<GitCloneResult>> {
    let url = format!("git+https://github.com/{}/{}.git", owner, repo);
    resolve_git_spec(&url, commit_ish, Some(repo)).await
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

    #[test]
    fn test_name_from_url_trailing_slash() {
        assert_eq!(name_from_url("git+https://github.com/user/repo/"), "repo");
        assert_eq!(
            name_from_url("git+https://github.com/user/repo.git/"),
            "repo"
        );
    }
}
