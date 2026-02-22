//! Git package resolver.
//!
//! Thin wrappers around ruborist's git clone functionality,
//! injecting the PM cache directory.

use anyhow::Result;

pub use utoo_ruborist::git::GitCloneResult;

use super::cache::get_cache_dir;

/// Resolve a git package spec by cloning the repo, checking out the ref,
/// reading package.json, and caching the result.
///
/// # Arguments
/// * `url` - Git URL, e.g. `git+https://github.com/user/repo.git`
/// * `commit_ish` - Optional branch, tag, or commit to check out
pub async fn resolve_git_spec(url: &str, commit_ish: Option<&str>) -> Result<GitCloneResult> {
    let cache_dir = get_cache_dir();
    utoo_ruborist::git::clone_repo(&cache_dir, url, commit_ish).await
}

/// Convert a `github:owner/repo` shorthand to a git+ URL and resolve.
pub async fn resolve_github_spec(
    owner: &str,
    repo: &str,
    commit_ish: Option<&str>,
) -> Result<GitCloneResult> {
    let url = format!("git+https://github.com/{}/{}.git", owner, repo);
    resolve_git_spec(&url, commit_ish).await
}

/// Check if a resolved URL is a git URL.
#[allow(dead_code)]
pub fn is_git_resolved(resolved: &str) -> bool {
    resolved.starts_with("git+") || resolved.starts_with("git://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_resolved() {
        assert!(is_git_resolved(
            "git+https://github.com/user/repo.git#abc123"
        ));
        assert!(is_git_resolved(
            "git+ssh://git@github.com/user/repo.git#abc123"
        ));
        assert!(is_git_resolved("git://github.com/user/repo.git"));
        assert!(!is_git_resolved(
            "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz"
        ));
        assert!(!is_git_resolved(""));
    }
}
