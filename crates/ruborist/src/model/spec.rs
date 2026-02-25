//! Package specification types for different dependency sources.
//!
//! Supports registry (semver), git, GitHub shorthand, local, and HTTP tarball specs.
//!
//! # Parsing
//! Specs implement [`FromStr`] so they can be parsed with `.parse()`:
//! ```
//! use utoo_ruborist::spec::PackageSpec;
//!
//! let spec: PackageSpec = "lodash@^4.17.0".parse().unwrap();
//! assert!(matches!(spec, PackageSpec::Registry { .. }));
//!
//! let spec: PackageSpec = "git+https://github.com/user/repo.git#main".parse().unwrap();
//! assert!(matches!(spec, PackageSpec::Git { .. }));
//! ```

use std::fmt;
use std::str::FromStr;

/// Protocol prefix for local dependency specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalProtocol {
    /// `file:../path`
    File,
    /// `link:../path`
    Link,
    /// `workspace:*`
    Workspace,
    /// `portal:../path`
    Portal,
}

impl LocalProtocol {
    /// Returns the protocol prefix string (e.g. `"file:"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file:",
            Self::Link => "link:",
            Self::Workspace => "workspace:",
            Self::Portal => "portal:",
        }
    }
}

impl fmt::Display for LocalProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display without trailing colon for readability
        match self {
            Self::File => write!(f, "file"),
            Self::Link => write!(f, "link"),
            Self::Workspace => write!(f, "workspace"),
            Self::Portal => write!(f, "portal"),
        }
    }
}

/// Typed representation of a package specification.
///
/// # Examples
/// ```
/// use utoo_ruborist::spec::PackageSpec;
///
/// let spec: PackageSpec = "lodash@^4.17.0".parse().unwrap();
/// assert!(matches!(spec, PackageSpec::Registry { .. }));
///
/// let spec: PackageSpec = "file:../local-pkg".parse().unwrap();
/// assert!(matches!(spec, PackageSpec::Local { .. }));
///
/// let spec: PackageSpec = "https://example.com/pkg.tgz".parse().unwrap();
/// assert!(matches!(spec, PackageSpec::HttpTarball { .. }));
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
    /// GitHub shorthand: `github:user/repo#ref` or bare `user/repo`
    GitHub {
        owner: String,
        repo: String,
        commit_ish: Option<String>,
    },
    /// Local dependency: `file:`, `link:`, `workspace:`, `portal:`
    Local {
        protocol: LocalProtocol,
        path: String,
    },
    /// HTTP tarball URL: `https://example.com/pkg.tgz`
    HttpTarball { url: String },
}

impl PackageSpec {
    /// Returns `true` if this is a non-registry spec (should skip registry preloading).
    pub fn is_registry(&self) -> bool {
        matches!(self, PackageSpec::Registry { .. })
    }
}

impl FromStr for PackageSpec {
    type Err = std::convert::Infallible;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        // git+https://... or git+ssh://... or git://...
        if raw.starts_with("git+") || raw.starts_with("git://") {
            let (url, commit_ish) = split_fragment(raw);
            return Ok(PackageSpec::Git {
                url: url.to_string(),
                commit_ish: commit_ish.map(String::from),
            });
        }

        // github:user/repo or github:user/repo#ref
        if let Some(rest) = raw.strip_prefix("github:") {
            let (path, commit_ish) = split_fragment(rest);
            if let Some((owner, repo)) = path.split_once('/') {
                return Ok(PackageSpec::GitHub {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    commit_ish: commit_ish.map(String::from),
                });
            }
            // `github:foo` without `/` — treat as Git URL so it doesn't
            // silently fall through to the registry resolver.
            return Ok(PackageSpec::Git {
                url: raw.to_string(),
                commit_ish: commit_ish.map(String::from),
            });
        }

        // Local specs: file:, link:, workspace:, portal:
        let local_protocols = [
            ("file:", LocalProtocol::File),
            ("link:", LocalProtocol::Link),
            ("workspace:", LocalProtocol::Workspace),
            ("portal:", LocalProtocol::Portal),
        ];
        for (prefix, protocol) in local_protocols {
            if let Some(path) = raw.strip_prefix(prefix) {
                return Ok(PackageSpec::Local {
                    protocol,
                    path: path.to_string(),
                });
            }
        }

        // HTTP tarball: https://example.com/pkg.tgz
        if is_http_tarball_spec(raw) {
            return Ok(PackageSpec::HttpTarball {
                url: raw.to_string(),
            });
        }

        // Bare GitHub shorthand: user/repo or user/repo#ref
        // npm treats "user/repo" (no protocol, not scoped) as github:user/repo
        if !raw.starts_with('@') && !raw.contains(':') {
            let (path, commit_ish) = split_fragment(raw);
            if let Some((owner, repo)) = path.split_once('/')
                && !owner.is_empty()
                && !repo.is_empty()
            {
                return Ok(PackageSpec::GitHub {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    commit_ish: commit_ish.map(String::from),
                });
            }
        }

        // Default: registry spec — use parse_package_spec for name@version splitting
        let (name, version_spec) = super::util::parse_package_spec(raw);
        Ok(PackageSpec::Registry {
            name: name.to_string(),
            version_spec: version_spec.to_string(),
        })
    }
}

/// Check if a spec string is an HTTP tarball URL.
fn is_http_tarball_spec(spec: &str) -> bool {
    if !(spec.starts_with("https://") || spec.starts_with("http://")) {
        return false;
    }
    // Only consider the path portion before any query (`?`) or fragment (`#`)
    let base = spec.split(['?', '#']).next().unwrap_or(spec);
    base.ends_with(".tgz") || base.ends_with(".tar.gz")
}

/// Split a string at the first `#` into (base, optional fragment).
///
/// An empty fragment (e.g. trailing `#`) is treated as `None`.
fn split_fragment(s: &str) -> (&str, Option<&str>) {
    match s.split_once('#') {
        Some((base, frag)) if !frag.is_empty() => (base, Some(frag)),
        Some((base, _)) => (base, None),
        None => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse a spec string into [`PackageSpec`] via `FromStr`.
    fn parse(s: &str) -> PackageSpec {
        s.parse().unwrap()
    }

    #[test]
    fn test_parse_registry_spec() {
        assert_eq!(
            parse("lodash@^4.17.0"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "^4.17.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_registry_spec_no_version() {
        assert_eq!(
            parse("lodash"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "*".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_scoped_registry_spec() {
        assert_eq!(
            parse("@scope/pkg@1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_git_https() {
        assert_eq!(
            parse("git+https://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_git_https_with_ref() {
        assert_eq!(
            parse("git+https://github.com/user/repo.git#main"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: Some("main".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_ssh() {
        assert_eq!(
            parse("git+ssh://git@github.com/user/repo.git#v1.0"),
            PackageSpec::Git {
                url: "git+ssh://git@github.com/user/repo.git".to_string(),
                commit_ish: Some("v1.0".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_protocol() {
        assert_eq!(
            parse("git://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_github_shorthand() {
        assert_eq!(
            parse("github:user/repo"),
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
            parse("github:user/repo#develop"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: Some("develop".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_trailing_hash() {
        assert_eq!(
            parse("git+https://github.com/user/repo.git#"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_github_no_slash() {
        let spec = parse("github:foo");
        assert!(matches!(spec, PackageSpec::Git { .. }));
    }

    #[test]
    fn test_parse_local_file() {
        assert_eq!(
            parse("file:../foo"),
            PackageSpec::Local {
                protocol: LocalProtocol::File,
                path: "../foo".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_local_link() {
        assert_eq!(
            parse("link:../foo"),
            PackageSpec::Local {
                protocol: LocalProtocol::Link,
                path: "../foo".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_local_workspace() {
        assert_eq!(
            parse("workspace:*"),
            PackageSpec::Local {
                protocol: LocalProtocol::Workspace,
                path: "*".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_local_portal() {
        assert_eq!(
            parse("portal:../foo"),
            PackageSpec::Local {
                protocol: LocalProtocol::Portal,
                path: "../foo".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball() {
        assert_eq!(
            parse("https://example.com/pkg.tgz"),
            PackageSpec::HttpTarball {
                url: "https://example.com/pkg.tgz".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball_with_query() {
        assert_eq!(
            parse("https://example.com/pkg.tgz?v=1.0"),
            PackageSpec::HttpTarball {
                url: "https://example.com/pkg.tgz?v=1.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball_tar_gz() {
        assert_eq!(
            parse("http://example.com/pkg.tar.gz"),
            PackageSpec::HttpTarball {
                url: "http://example.com/pkg.tar.gz".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_bare_github_shorthand() {
        assert_eq!(
            parse("user/repo"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_bare_github_shorthand_with_ref() {
        assert_eq!(
            parse("user/repo#develop"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: Some("develop".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_scoped_not_github() {
        assert_eq!(
            parse("@scope/pkg@1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_is_http_tarball_spec() {
        assert!(is_http_tarball_spec("https://example.com/pkg.tgz"));
        assert!(is_http_tarball_spec("http://example.com/pkg.tar.gz"));
        assert!(is_http_tarball_spec("https://example.com/pkg.tgz?v=1.0"));
        assert!(is_http_tarball_spec(
            "https://example.com/pkg.tar.gz#download"
        ));
        assert!(!is_http_tarball_spec("https://example.com/pkg"));
        assert!(!is_http_tarball_spec("lodash@^4.0.0"));
    }
}
