use std::path::Path;

/// Get the directory name from a path, falling back to "package".
pub fn dir_name(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("package")
        .to_string()
}

/// Detect the git author from `git config user.name` and `git config user.email`.
pub fn detect_author() -> String {
    let name = git_config("user.name").unwrap_or_default();
    let email = git_config("user.email").unwrap_or_default();

    match (name.is_empty(), email.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name,
        (true, false) => format!("<{email}>"),
        (false, false) => format!("{name} <{email}>"),
    }
}

/// Detect the git remote origin URL for the given directory.
pub fn detect_repository(cwd: &Path) -> String {
    std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Normalize a git remote URL to npm-conventional format.
///
/// - `git@host:user/repo.git` → `git+ssh://git@host/user/repo.git`
/// - `https://host/user/repo.git` → `git+https://host/user/repo.git`
/// - `http://host/user/repo.git` → `git+http://host/user/repo.git`
/// - Already prefixed with `git+` or other formats → returned as-is
pub fn normalize_url(url: &str) -> String {
    if url.starts_with("git+") {
        return url.to_string();
    }

    // SSH shorthand: git@host:user/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("git+ssh://git@{host}/{path}");
        }
    }

    // https:// or http://
    if url.starts_with("https://") || url.starts_with("http://") {
        return format!("git+{url}");
    }

    url.to_string()
}

fn git_config(key: &str) -> Option<String> {
    std::process::Command::new("git")
        .args(["config", key])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_name() {
        assert_eq!(dir_name(Path::new("/home/user/my-project")), "my-project");
    }

    #[test]
    fn test_dir_name_root() {
        assert_eq!(dir_name(Path::new("/")), "package");
    }

    #[test]
    fn test_normalize_url_ssh_shorthand() {
        assert_eq!(
            normalize_url("git@github.com:user/repo.git"),
            "git+ssh://git@github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_url_https() {
        assert_eq!(
            normalize_url("https://github.com/user/repo.git"),
            "git+https://github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_url_http() {
        assert_eq!(
            normalize_url("http://github.com/user/repo.git"),
            "git+http://github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_url_already_prefixed() {
        let url = "git+ssh://git@github.com/user/repo.git";
        assert_eq!(normalize_url(url), url);

        let url2 = "git+https://github.com/user/repo.git";
        assert_eq!(normalize_url(url2), url2);
    }

    #[test]
    fn test_normalize_url_other_format() {
        let url = "file:///path/to/repo";
        assert_eq!(normalize_url(url), url);
    }
}
