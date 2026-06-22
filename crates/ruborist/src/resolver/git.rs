//! Git clone backend powered by the system `git` executable.
//!
//! Git packages are stored under `<cache_dir>/<name>/<commit_sha>/`, the same
//! `<name>/<key>/` layout used by registry tarballs, so `utoo clean` works
//! uniformly without any git-specific knowledge.
//!
//! Remote transport is delegated to `git` instead of linking gitoxide's
//! reqwest/rustls transport into the package-manager binary. The blocking
//! clone and filesystem copy are run in a `tokio::task::spawn_blocking` thread
//! so the async executor is never blocked.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
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

fn redact_secret(text: &str, token: Option<&str>) -> String {
    match token {
        Some(token) if !token.is_empty() => text.replace(token, "<redacted>"),
        _ => text.to_string(),
    }
}

fn run_git(
    args: &[&str],
    cwd: Option<&Path>,
    op: &str,
    public_url: &str,
    token: Option<&str>,
) -> Result<String> {
    let mut command = Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.args(args);

    let output = command
        .output()
        .with_context(|| format!("failed to execute git while running {op}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = redact_secret(&stderr, token);
        let mut msg = format!("{op} failed for '{public_url}'");
        if let Some(code) = output.status.code() {
            msg.push_str(&format!(" (exit code {code})"));
        }
        if !stderr.trim().is_empty() {
            msg.push_str(": ");
            msg.push_str(stderr.trim());
        }
        if token.is_none()
            && (public_url.contains("github.com") || public_url.contains("gitlab.com"))
        {
            msg.push_str(
                "\n\nTip: set GITHUB_TOKEN or GH_TOKEN for private repos / higher rate limits",
            );
        }
        return Err(anyhow!(msg));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_pkg_manifest(checkout_dir: &Path) -> Result<CoreVersionManifest> {
    let pkg_bytes = std::fs::read(checkout_dir.join("package.json"))
        .context("package.json not found in git repo root (does the repo have a package.json?)")?;
    serde_json::from_slice(&pkg_bytes).context("Failed to parse package.json from git checkout")
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

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn symlink_target_stays_in_root(root_dest: &Path, link_path: &Path, target: &Path) -> bool {
    let link_parent = link_path.parent().unwrap_or(root_dest);
    let resolved = if target.is_absolute() {
        target.to_path_buf()
    } else {
        link_parent.join(target)
    };
    normalize_lexically(&resolved).starts_with(normalize_lexically(root_dest))
}

/// Recursively copy a checked-out git worktree to a directory on disk.
///
/// `root_dest` is the top-level extraction directory; symlinks that would
/// escape it are skipped to prevent path-traversal attacks.
fn copy_worktree_to_dir(src: &Path, dest: &Path, root_dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;

    for entry in
        std::fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))?
    {
        let entry = entry?;
        let file_name = entry.file_name();
        if file_name == ".git" {
            continue;
        }

        let src_path = entry.path();
        let dest_path = dest.join(&file_name);
        let metadata = std::fs::symlink_metadata(&src_path)?;
        let file_type = metadata.file_type();

        if file_type.is_dir() {
            copy_worktree_to_dir(&src_path, &dest_path, root_dest)?;
        } else if file_type.is_symlink() {
            let target = std::fs::read_link(&src_path)
                .with_context(|| format!("failed to read symlink {}", src_path.display()))?;
            if !symlink_target_stays_in_root(root_dest, &dest_path, &target) {
                tracing::debug!(
                    "Skipping symlink {} -> {} (escapes extraction dir)",
                    dest_path.display(),
                    target.display()
                );
                continue;
            }

            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &dest_path)
                .with_context(|| format!("failed to create symlink {}", dest_path.display()))?;
            #[cfg(not(unix))]
            {
                let _ = target;
                tracing::debug!(
                    "Skipping symlink {} (symlink cache extraction is unsupported on this platform)",
                    dest_path.display()
                );
            }
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    src_path.display(),
                    dest_path.display()
                )
            })?;

            #[cfg(unix)]
            {
                let perms = std::fs::Permissions::from_mode(metadata.permissions().mode() & 0o777);
                std::fs::set_permissions(&dest_path, perms).with_context(|| {
                    format!("failed to set permissions on {}", dest_path.display())
                })?;
            }
        }
    }

    Ok(())
}

fn is_full_sha(commit_ish: Option<&str>) -> bool {
    commit_ish.is_some_and(|c| {
        (c.len() == 40 || c.len() == 64) && c.chars().all(|ch| ch.is_ascii_hexdigit())
    })
}

/// Blocking core that does the actual clone + extraction via system git.
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

    let is_full_sha = is_full_sha(commit_ish);

    // Early cache check for full SHA specs. The package name is known at call
    // site so we check the exact path rather than scanning directories.
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
                }
            }
        }
    }

    // Capture auth token once; reused for URL injection and error redaction.
    let token = github_auth_token();
    let effective_url = match &token {
        Some(t) => try_inject_token_into_https_url(clone_url, t),
        None => clone_url.to_string(),
    };

    let temp_dir = tempfile::tempdir().context("Failed to create temp directory for git clone")?;
    let checkout_dir = temp_dir.path().join("repo");
    let checkout_str = checkout_dir
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 temporary checkout path"))?;

    let mut clone_args = vec!["clone", "--quiet"];
    if is_full_sha {
        clone_args.push("--no-checkout");
    } else {
        clone_args.extend(["--depth", "1"]);
        if let Some(ref_name) = commit_ish {
            clone_args.extend(["--branch", ref_name]);
        }
    }
    clone_args.extend(["--", effective_url.as_str(), checkout_str]);

    run_git(&clone_args, None, "git clone", clone_url, token.as_deref())?;

    if let Some(sha) = commit_ish
        && is_full_sha
    {
        run_git(
            &["checkout", "--quiet", sha],
            Some(&checkout_dir),
            "git checkout",
            clone_url,
            token.as_deref(),
        )?;
    }

    let commit_hex = run_git(
        &["rev-parse", "HEAD"],
        Some(&checkout_dir),
        "git rev-parse",
        clone_url,
        token.as_deref(),
    )?;

    let mut manifest = read_pkg_manifest(&checkout_dir)?;
    let pinned_url = format!("{}#{}", resolved_url, commit_hex);
    finalize_non_registry_manifest(&mut manifest, pinned_url.clone())?;

    let package_dir = cache_dir.join(&manifest.name).join(&commit_hex);
    if package_dir.join("_resolved").exists() {
        return Ok(GitCloneResult::new(package_dir, pinned_url, manifest));
    }

    commit_cache_dir_atomic(&package_dir, |stage| {
        copy_worktree_to_dir(&checkout_dir, stage, stage)
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
/// guarantee survives; re-parsing from a raw string would discard it.
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
    use std::process::Command;

    use super::*;

    fn run_git_test(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .status()
            .expect("git should be executable for git resolver tests");
        assert!(status.success(), "git {args:?} failed with {status}");
    }

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

    #[test]
    fn symlink_escape_check_rejects_parent_escape() {
        let root = Path::new("/tmp/cache/pkg");
        let link = root.join("nested/link");
        assert!(!symlink_target_stays_in_root(
            root,
            &link,
            Path::new("../../outside")
        ));
        assert!(symlink_target_stays_in_root(
            root,
            &link,
            Path::new("../target")
        ));
    }

    #[test]
    fn full_sha_detection_accepts_sha1_and_sha256() {
        assert!(is_full_sha(Some(
            "0123456789abcdef0123456789abcdef01234567"
        )));
        assert!(is_full_sha(Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        )));
        assert!(!is_full_sha(Some("main")));
        assert!(!is_full_sha(Some(
            "0123456789abcdef0123456789abcdef0123456g"
        )));
    }

    #[tokio::test]
    async fn ensure_repo_cached_clones_local_repo() {
        let repo = tempfile::tempdir().unwrap();
        run_git_test(repo.path(), &["init", "--quiet"]);
        run_git_test(repo.path(), &["config", "user.email", "utoo@example.test"]);
        run_git_test(repo.path(), &["config", "user.name", "Utoo Test"]);

        std::fs::write(
            repo.path().join("package.json"),
            r#"{"name":"demo-git-package","version":"1.2.3"}"#,
        )
        .unwrap();
        std::fs::write(repo.path().join("index.js"), "module.exports = 1;\n").unwrap();
        run_git_test(repo.path(), &["add", "."]);
        run_git_test(repo.path(), &["commit", "--quiet", "-m", "init"]);

        let cache_dir = tempfile::tempdir().unwrap();
        let clone_cache = GitCloneCache::new();
        let repo_url = repo.path().to_str().unwrap();
        let result = ensure_repo_cached(
            cache_dir.path(),
            repo_url,
            None,
            "demo-git-package",
            &clone_cache,
        )
        .await
        .unwrap();

        assert_eq!(result.name, "demo-git-package");
        assert_eq!(result.version, "1.2.3");
        assert!(result.path.join("_resolved").exists());
        assert!(result.path.join("package.json").exists());
        assert!(result.path.join("index.js").exists());
        assert!(result.resolved_url.starts_with(repo_url));
    }
}
