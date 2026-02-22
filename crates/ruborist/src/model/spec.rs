//! Package specification types for different dependency sources.
//!
//! Supports registry (semver), git, GitHub shorthand, HTTP tarball, and file specs.

/// Typed representation of a package specification.
#[derive(Debug, Clone, PartialEq)]
pub enum PackageSpec {
    /// Registry semver spec: `lodash@^4.17.0`
    Registry { name: String, version_spec: String },
    /// Git URL: `git+https://github.com/user/repo.git#ref`
    Git {
        url: String,
        commit_ish: Option<String>,
    },
    /// GitHub shorthand: `github:user/repo#ref`
    GitHub {
        owner: String,
        repo: String,
        commit_ish: Option<String>,
    },
    /// HTTP tarball: `https://example.com/pkg.tgz` (future)
    Http { url: String },
    /// File directory: `file:../path` (future)
    FileDir { path: String },
    /// File tarball: `file:../path.tgz` (future)
    FileTarball { path: String },
}

/// Check if a spec string is an HTTP tarball URL.
pub fn is_http_tarball_spec(spec: &str) -> bool {
    (spec.starts_with("https://") || spec.starts_with("http://"))
        && (spec.ends_with(".tgz") || spec.ends_with(".tar.gz"))
}

/// Parse a CLI argument into a typed `PackageSpec`.
///
/// Recognizes git, GitHub shorthand, HTTP tarball, and file specs before
/// falling back to the standard `name@version` registry format.
///
/// # Examples
/// ```
/// use utoo_ruborist::model::spec::{PackageSpec, parse_cli_spec};
///
/// let spec = parse_cli_spec("lodash@^4.17.0");
/// assert!(matches!(spec, PackageSpec::Registry { .. }));
///
/// let spec = parse_cli_spec("git+https://github.com/user/repo.git#main");
/// assert!(matches!(spec, PackageSpec::Git { .. }));
///
/// let spec = parse_cli_spec("github:user/repo#v1.0");
/// assert!(matches!(spec, PackageSpec::GitHub { .. }));
/// ```
pub fn parse_cli_spec(raw: &str) -> PackageSpec {
    // git+https://... or git+ssh://... or git://...
    if raw.starts_with("git+") || raw.starts_with("git://") {
        let (url, commit_ish) = split_fragment(raw);
        return PackageSpec::Git {
            url: url.to_string(),
            commit_ish: commit_ish.map(String::from),
        };
    }

    // github:user/repo or github:user/repo#ref
    if let Some(rest) = raw.strip_prefix("github:") {
        let (path, commit_ish) = split_fragment(rest);
        if let Some((owner, repo)) = path.split_once('/') {
            return PackageSpec::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
                commit_ish: commit_ish.map(String::from),
            };
        }
    }

    // HTTP tarball: https://example.com/pkg.tgz
    if is_http_tarball_spec(raw) {
        return PackageSpec::Http {
            url: raw.to_string(),
        };
    }

    // file: specs
    if let Some(path) = raw.strip_prefix("file:") {
        if path.ends_with(".tgz") || path.ends_with(".tar.gz") {
            return PackageSpec::FileTarball {
                path: path.to_string(),
            };
        }
        return PackageSpec::FileDir {
            path: path.to_string(),
        };
    }

    // Default: registry spec — use parse_package_spec for name@version splitting
    let (name, version_spec) = super::util::parse_package_spec(raw);
    PackageSpec::Registry {
        name: name.to_string(),
        version_spec: version_spec.to_string(),
    }
}

/// Split a string at the first `#` into (base, optional fragment).
fn split_fragment(s: &str) -> (&str, Option<&str>) {
    match s.find('#') {
        Some(pos) => (&s[..pos], Some(&s[pos + 1..])),
        None => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_registry_spec() {
        assert_eq!(
            parse_cli_spec("lodash@^4.17.0"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "^4.17.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_registry_spec_no_version() {
        assert_eq!(
            parse_cli_spec("lodash"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "*".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_scoped_registry_spec() {
        assert_eq!(
            parse_cli_spec("@scope/pkg@1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_git_https() {
        assert_eq!(
            parse_cli_spec("git+https://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_git_https_with_ref() {
        assert_eq!(
            parse_cli_spec("git+https://github.com/user/repo.git#main"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: Some("main".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_ssh() {
        assert_eq!(
            parse_cli_spec("git+ssh://git@github.com/user/repo.git#v1.0"),
            PackageSpec::Git {
                url: "git+ssh://git@github.com/user/repo.git".to_string(),
                commit_ish: Some("v1.0".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_github_shorthand() {
        assert_eq!(
            parse_cli_spec("github:user/repo"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_github_shorthand_with_ref() {
        assert_eq!(
            parse_cli_spec("github:user/repo#develop"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: Some("develop".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball() {
        assert_eq!(
            parse_cli_spec("https://example.com/pkg-1.0.0.tgz"),
            PackageSpec::Http {
                url: "https://example.com/pkg-1.0.0.tgz".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_file_dir() {
        assert_eq!(
            parse_cli_spec("file:../my-lib"),
            PackageSpec::FileDir {
                path: "../my-lib".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_file_tarball() {
        assert_eq!(
            parse_cli_spec("file:../pkg-1.0.0.tgz"),
            PackageSpec::FileTarball {
                path: "../pkg-1.0.0.tgz".to_string(),
            }
        );
    }

    #[test]
    fn test_is_http_tarball_spec() {
        assert!(is_http_tarball_spec("https://example.com/pkg.tgz"));
        assert!(is_http_tarball_spec("http://example.com/pkg.tar.gz"));
        assert!(!is_http_tarball_spec("https://example.com/pkg"));
        assert!(!is_http_tarball_spec("lodash@^4.0.0"));
    }
}
