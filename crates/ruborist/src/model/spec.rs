//! Package specification types for different dependency sources.
//!
//! Supports registry (semver), git, and GitHub shorthand specs.

/// Typed representation of a package specification.
///
/// # Examples
/// ```
/// use utoo_ruborist::spec::PackageSpec;
///
/// let spec = PackageSpec::parse("lodash@^4.17.0");
/// assert!(matches!(spec, PackageSpec::Registry { .. }));
///
/// let spec = PackageSpec::parse("git+https://github.com/user/repo.git#main");
/// assert!(matches!(spec, PackageSpec::Git { .. }));
///
/// let spec = PackageSpec::parse("github:user/repo#v1.0");
/// assert!(matches!(spec, PackageSpec::GitHub { .. }));
/// ```
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
}

impl PackageSpec {
    /// Parse a raw spec string into a typed [`PackageSpec`].
    ///
    /// Recognizes git and GitHub shorthand specs before falling back to
    /// the standard `name@version` registry format.
    ///
    /// # Examples
    /// ```
    /// use utoo_ruborist::spec::PackageSpec;
    ///
    /// let spec = PackageSpec::parse("lodash@^4.17.0");
    /// assert_eq!(spec, PackageSpec::Registry {
    ///     name: "lodash".to_string(),
    ///     version_spec: "^4.17.0".to_string(),
    /// });
    ///
    /// let spec = PackageSpec::parse("git+https://github.com/user/repo.git#main");
    /// assert_eq!(spec, PackageSpec::Git {
    ///     url: "git+https://github.com/user/repo.git".to_string(),
    ///     commit_ish: Some("main".to_string()),
    /// });
    /// ```
    pub fn parse(raw: &str) -> PackageSpec {
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

        // Default: registry spec — use parse_package_spec for name@version splitting
        let (name, version_spec) = super::util::parse_package_spec(raw);
        PackageSpec::Registry {
            name: name.to_string(),
            version_spec: version_spec.to_string(),
        }
    }
}

/// Check if a spec string is an HTTP tarball URL.
///
/// Useful for routing specs that look like HTTP tarballs away from
/// the standard registry/git resolution paths.
pub fn is_http_tarball_spec(spec: &str) -> bool {
    (spec.starts_with("https://") || spec.starts_with("http://"))
        && (spec.ends_with(".tgz") || spec.ends_with(".tar.gz"))
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
            PackageSpec::parse("lodash@^4.17.0"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "^4.17.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_registry_spec_no_version() {
        assert_eq!(
            PackageSpec::parse("lodash"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "*".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_scoped_registry_spec() {
        assert_eq!(
            PackageSpec::parse("@scope/pkg@1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_git_https() {
        assert_eq!(
            PackageSpec::parse("git+https://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_git_https_with_ref() {
        assert_eq!(
            PackageSpec::parse("git+https://github.com/user/repo.git#main"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: Some("main".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_ssh() {
        assert_eq!(
            PackageSpec::parse("git+ssh://git@github.com/user/repo.git#v1.0"),
            PackageSpec::Git {
                url: "git+ssh://git@github.com/user/repo.git".to_string(),
                commit_ish: Some("v1.0".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_protocol() {
        assert_eq!(
            PackageSpec::parse("git://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_github_shorthand() {
        assert_eq!(
            PackageSpec::parse("github:user/repo"),
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
            PackageSpec::parse("github:user/repo#develop"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: Some("develop".to_string()),
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
