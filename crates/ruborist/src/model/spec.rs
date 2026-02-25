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

// ---------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------

/// Protocol prefix detected in a dependency spec string.
///
/// Implements [`FromStr`] so a raw spec can be probed for its protocol:
/// ```
/// use utoo_ruborist::spec::Protocol;
///
/// let p: Protocol = "https://example.com/pkg.tgz".parse().unwrap();
/// assert_eq!(p, Protocol::Http);
///
/// assert!("lodash@^4".parse::<Protocol>().is_err()); // registry has no protocol prefix
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `file:` — local file dependency
    File,
    /// `link:` — local symlink dependency
    Link,
    /// `workspace:` — workspace dependency
    Workspace,
    /// `portal:` — portal dependency
    Portal,
    /// `git+https://`, `git+ssh://`, `git://` — git repository
    Git,
    /// `github:` — GitHub shorthand
    GitHub,
    /// `http://`, `https://` — HTTP URL (may be tarball)
    Http,
}

/// All known protocol prefixes, in detection order (most specific first).
const PROTOCOL_PREFIXES: &[(Protocol, &[&str])] = &[
    (Protocol::Git, &["git+", "git://"]),
    (Protocol::GitHub, &["github:"]),
    (Protocol::File, &["file:"]),
    (Protocol::Link, &["link:"]),
    (Protocol::Workspace, &["workspace:"]),
    (Protocol::Portal, &["portal:"]),
    (Protocol::Http, &["https://", "http://"]),
];

impl Protocol {
    /// Try to detect a protocol prefix from a raw spec string.
    ///
    /// Returns the protocol and the remaining string after the matched prefix,
    /// or `None` if no known protocol prefix is found.
    pub fn parse_prefix(spec: &str) -> Option<(Self, &str)> {
        for &(proto, prefixes) in PROTOCOL_PREFIXES {
            for prefix in prefixes {
                if let Some(rest) = spec.strip_prefix(prefix) {
                    return Some((proto, rest));
                }
            }
        }
        None
    }

    /// Returns `true` if this is a local protocol (`file`, `link`, `workspace`, `portal`).
    pub fn is_local(self) -> bool {
        matches!(
            self,
            Self::File | Self::Link | Self::Workspace | Self::Portal
        )
    }
}

/// Error returned when a string has no recognizable protocol prefix.
#[derive(Debug, Clone, Copy)]
pub struct ParseProtocolError;

impl fmt::Display for ParseProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no known protocol prefix")
    }
}

impl std::error::Error for ParseProtocolError {}

impl FromStr for Protocol {
    type Err = ParseProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_prefix(s)
            .map(|(p, _)| p)
            .ok_or(ParseProtocolError)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Link => write!(f, "link"),
            Self::Workspace => write!(f, "workspace"),
            Self::Portal => write!(f, "portal"),
            Self::Git => write!(f, "git"),
            Self::GitHub => write!(f, "github"),
            Self::Http => write!(f, "http"),
        }
    }
}

// ---------------------------------------------------------------------------
// PackageSpec
// ---------------------------------------------------------------------------

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
    Local { protocol: Protocol, path: String },
    /// HTTP tarball URL: `https://example.com/pkg.tgz`
    HttpTarball { url: String },
}

impl PackageSpec {
    /// Returns `true` if this is a registry spec.
    pub fn is_registry(&self) -> bool {
        matches!(self, PackageSpec::Registry { .. })
    }
}

impl FromStr for PackageSpec {
    type Err = std::convert::Infallible;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match Protocol::parse_prefix(raw) {
            Some((Protocol::Git, _)) => {
                let (url, commit_ish) = split_fragment(raw);
                Ok(PackageSpec::Git {
                    url: url.to_string(),
                    commit_ish: commit_ish.map(String::from),
                })
            }
            Some((Protocol::GitHub, rest)) => {
                let (path, commit_ish) = split_fragment(rest);
                if let Some((owner, repo)) = path.split_once('/') {
                    Ok(PackageSpec::GitHub {
                        owner: owner.to_string(),
                        repo: repo.to_string(),
                        commit_ish: commit_ish.map(String::from),
                    })
                } else {
                    // `github:foo` without `/` — treat as Git URL so it doesn't
                    // silently fall through to the registry resolver.
                    Ok(PackageSpec::Git {
                        url: raw.to_string(),
                        commit_ish: commit_ish.map(String::from),
                    })
                }
            }
            Some((proto, rest)) if proto.is_local() => Ok(PackageSpec::Local {
                protocol: proto,
                path: rest.to_string(),
            }),
            Some((Protocol::Http, _)) if has_tarball_extension(raw) => {
                Ok(PackageSpec::HttpTarball {
                    url: raw.to_string(),
                })
            }
            _ => {
                // Bare GitHub shorthand: user/repo or user/repo#ref
                // npm treats "user/repo" (no protocol, not scoped) as github:user/repo
                if !raw.starts_with('@') {
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

                // Default: registry spec
                let (name, version_spec) = super::util::parse_package_spec(raw);
                Ok(PackageSpec::Registry {
                    name: name.to_string(),
                    version_spec: version_spec.to_string(),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a URL path ends with a tarball extension (`.tgz` or `.tar.gz`),
/// ignoring query parameters and fragments.
fn has_tarball_extension(url: &str) -> bool {
    let base = url.split(['?', '#']).next().unwrap_or(url);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(s: &str) -> PackageSpec {
        s.parse().unwrap()
    }

    // -- Protocol --

    #[test]
    fn test_protocol_from_str() {
        assert_eq!("file:../foo".parse::<Protocol>().unwrap(), Protocol::File);
        assert_eq!("link:../foo".parse::<Protocol>().unwrap(), Protocol::Link);
        assert_eq!(
            "workspace:*".parse::<Protocol>().unwrap(),
            Protocol::Workspace
        );
        assert_eq!(
            "portal:../foo".parse::<Protocol>().unwrap(),
            Protocol::Portal
        );
        assert_eq!(
            "git+https://example.com".parse::<Protocol>().unwrap(),
            Protocol::Git
        );
        assert_eq!(
            "git://example.com".parse::<Protocol>().unwrap(),
            Protocol::Git
        );
        assert_eq!(
            "github:user/repo".parse::<Protocol>().unwrap(),
            Protocol::GitHub
        );
        assert_eq!(
            "https://example.com/pkg.tgz".parse::<Protocol>().unwrap(),
            Protocol::Http
        );
        assert_eq!(
            "http://example.com/pkg.tgz".parse::<Protocol>().unwrap(),
            Protocol::Http
        );
    }

    #[test]
    fn test_protocol_from_str_no_match() {
        assert!("lodash@^4".parse::<Protocol>().is_err());
        assert!("latest".parse::<Protocol>().is_err());
        assert!("user/repo".parse::<Protocol>().is_err());
    }

    #[test]
    fn test_protocol_is_local() {
        assert!(Protocol::File.is_local());
        assert!(Protocol::Link.is_local());
        assert!(Protocol::Workspace.is_local());
        assert!(Protocol::Portal.is_local());
        assert!(!Protocol::Git.is_local());
        assert!(!Protocol::GitHub.is_local());
        assert!(!Protocol::Http.is_local());
    }

    // -- PackageSpec: Registry --

    #[test]
    fn test_parse_registry_spec() {
        assert_eq!(
            spec("lodash@^4.17.0"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "^4.17.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_registry_spec_no_version() {
        assert_eq!(
            spec("lodash"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "*".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_scoped_registry_spec() {
        assert_eq!(
            spec("@scope/pkg@1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "1.0.0".to_string(),
            }
        );
    }

    // -- PackageSpec: Git --

    #[test]
    fn test_parse_git_https() {
        assert_eq!(
            spec("git+https://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_git_https_with_ref() {
        assert_eq!(
            spec("git+https://github.com/user/repo.git#main"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: Some("main".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_ssh() {
        assert_eq!(
            spec("git+ssh://git@github.com/user/repo.git#v1.0"),
            PackageSpec::Git {
                url: "git+ssh://git@github.com/user/repo.git".to_string(),
                commit_ish: Some("v1.0".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_git_protocol() {
        assert_eq!(
            spec("git://github.com/user/repo.git"),
            PackageSpec::Git {
                url: "git://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    #[test]
    fn test_parse_git_trailing_hash() {
        assert_eq!(
            spec("git+https://github.com/user/repo.git#"),
            PackageSpec::Git {
                url: "git+https://github.com/user/repo.git".to_string(),
                commit_ish: None,
            }
        );
    }

    // -- PackageSpec: GitHub --

    #[test]
    fn test_parse_github_shorthand() {
        assert_eq!(
            spec("github:user/repo"),
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
            spec("github:user/repo#develop"),
            PackageSpec::GitHub {
                owner: "user".to_string(),
                repo: "repo".to_string(),
                commit_ish: Some("develop".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_github_no_slash() {
        assert!(matches!(spec("github:foo"), PackageSpec::Git { .. }));
    }

    #[test]
    fn test_parse_bare_github_shorthand() {
        assert_eq!(
            spec("user/repo"),
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
            spec("user/repo#develop"),
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
            spec("@scope/pkg@1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "1.0.0".to_string(),
            }
        );
    }

    // -- PackageSpec: Local --

    #[test]
    fn test_parse_local_file() {
        assert_eq!(
            spec("file:../foo"),
            PackageSpec::Local {
                protocol: Protocol::File,
                path: "../foo".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_local_link() {
        assert_eq!(
            spec("link:../foo"),
            PackageSpec::Local {
                protocol: Protocol::Link,
                path: "../foo".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_local_workspace() {
        assert_eq!(
            spec("workspace:*"),
            PackageSpec::Local {
                protocol: Protocol::Workspace,
                path: "*".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_local_portal() {
        assert_eq!(
            spec("portal:../foo"),
            PackageSpec::Local {
                protocol: Protocol::Portal,
                path: "../foo".to_string(),
            }
        );
    }

    // -- PackageSpec: HttpTarball --

    #[test]
    fn test_parse_http_tarball() {
        assert_eq!(
            spec("https://example.com/pkg.tgz"),
            PackageSpec::HttpTarball {
                url: "https://example.com/pkg.tgz".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball_with_query() {
        assert_eq!(
            spec("https://example.com/pkg.tgz?v=1.0"),
            PackageSpec::HttpTarball {
                url: "https://example.com/pkg.tgz?v=1.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball_tar_gz() {
        assert_eq!(
            spec("http://example.com/pkg.tar.gz"),
            PackageSpec::HttpTarball {
                url: "http://example.com/pkg.tar.gz".to_string(),
            }
        );
    }

    // -- has_tarball_extension --

    #[test]
    fn test_has_tarball_extension() {
        assert!(has_tarball_extension("https://example.com/pkg.tgz"));
        assert!(has_tarball_extension("http://example.com/pkg.tar.gz"));
        assert!(has_tarball_extension("https://example.com/pkg.tgz?v=1.0"));
        assert!(has_tarball_extension(
            "https://example.com/pkg.tar.gz#download"
        ));
        assert!(!has_tarball_extension("https://example.com/pkg"));
        assert!(!has_tarball_extension("lodash@^4.0.0"));
    }
}
