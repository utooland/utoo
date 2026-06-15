//! Package specification types.
//!
//! [`PackageSpec::from()`] parses a spec string into a typed enum for the
//! resolver. Parsing is infallible:
//!
//! ```
//! use utoo_ruborist::spec::{PackageSpec, SpecStr};
//!
//! let spec = "lodash@^4.17.0".parse_spec();
//! assert!(matches!(spec, PackageSpec::Registry { .. }));
//!
//! let spec = PackageSpec::from("git+https://github.com/user/repo.git#main");
//! assert!(matches!(spec, PackageSpec::Git { .. }));
//! ```
//!
//! ## Supported protocols
//!
//! | Protocol        | PackageSpec variant | Notes                           |
//! |-----------------|---------------------|---------------------------------|
//! | `catalog:`      | `Local`             | Resolved in `process_dependency`|
//! | `workspace:`    | `Local`             | Resolved during graph init      |
//! | `git+`/`git://` | `Git`              | Resolved by builder             |
//! | `github:`       | `GitHub`            | Resolved by builder             |
//! | `file:`/`link:` | `Local`             | Resolved by builder             |
//! | `http:`/`https:`| `Http`              | Resolved by builder             |
//! | `npm:`          | `Registry`          | Alias: `npm:lodash@^4`          |
//! | (semver)        | `Registry`          | Resolved by registry            |

use std::collections::HashMap;
use std::str::FromStr;

use crate::model::util::{PackageNameStr, parse_package_spec};

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
    /// `catalog:` — catalog protocol reference (resolved before registry lookup)
    Catalog,
    /// `npm:` — npm alias (e.g., `npm:lodash@^4.17.0`)
    NpmAlias,
}

impl Protocol {
    /// Strip a known protocol prefix from a raw spec string.
    ///
    /// Returns the protocol and the remainder after the prefix,
    /// or `None` if no known protocol prefix is found.
    pub fn strip_prefix(spec: &str) -> Option<(Self, &str)> {
        // Flat table: one row per prefix, most-specific entries first.
        const PREFIXES: &[(Protocol, &str)] = &[
            (Protocol::Git, "git+"),
            (Protocol::Git, "git://"),
            (Protocol::GitHub, "github:"),
            (Protocol::Catalog, "catalog:"),
            (Protocol::File, "file:"),
            (Protocol::Link, "link:"),
            (Protocol::Workspace, "workspace:"),
            (Protocol::Portal, "portal:"),
            (Protocol::NpmAlias, "npm:"),
            (Protocol::Http, "https://"),
            (Protocol::Http, "http://"),
        ];
        PREFIXES
            .iter()
            .find_map(|&(proto, pfx)| spec.strip_prefix(pfx).map(|rest| (proto, rest)))
    }

    /// Returns `true` if this is a local protocol (`file`, `link`, `workspace`, `portal`).
    pub fn is_local(self) -> bool {
        match self {
            Self::File | Self::Link | Self::Workspace | Self::Portal => true,
            Self::Git | Self::GitHub | Self::Http | Self::Catalog | Self::NpmAlias => false,
        }
    }
}

/// Error returned when a string has no recognizable protocol prefix.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("unsupported protocol prefix")]
pub struct ParseProtocolError;

impl FromStr for Protocol {
    type Err = ParseProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::strip_prefix(s)
            .map(|(p, _)| p)
            .ok_or(ParseProtocolError)
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Link => write!(f, "link"),
            Self::Workspace => write!(f, "workspace"),
            Self::Portal => write!(f, "portal"),
            Self::Git => write!(f, "git"),
            Self::GitHub => write!(f, "github"),
            Self::Http => write!(f, "http"),
            Self::Catalog => write!(f, "catalog"),
            Self::NpmAlias => write!(f, "npm"),
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
/// use utoo_ruborist::spec::{PackageSpec, SpecStr};
///
/// let spec = "lodash@^4.17.0".parse_spec();
/// assert!(matches!(spec, PackageSpec::Registry { .. }));
///
/// let spec = PackageSpec::from("file:../local-pkg");
/// assert!(matches!(spec, PackageSpec::Local { .. }));
///
/// let spec = PackageSpec::from("https://example.com/pkg.tgz");
/// assert!(matches!(spec, PackageSpec::Http { .. }));
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
    /// HTTP URL dependency: `https://example.com/pkg.tgz`
    Http { url: String },
}

impl PackageSpec {
    /// Returns `true` if this is a registry spec.
    pub fn is_registry(&self) -> bool {
        matches!(self, PackageSpec::Registry { .. })
    }

    /// Return the clone-ready URL for a git spec, stripping the `git+` prefix.
    ///
    /// Returns `None` for non-git variants.
    pub fn clone_url(&self) -> Option<&str> {
        match self {
            PackageSpec::Git { url, .. } => Some(url.strip_prefix("git+").unwrap_or(url)),
            _ => None,
        }
    }
}

/// Coarse cache-layout classification of a resolved package, derived from the
/// dependency's [`PackageSpec`].
///
/// Each variant maps to a distinct cache-slot layout, so the installer must
/// route download/extract and seeded-slot lookup by this — **not** by
/// re-parsing the resolved tarball URL. A registry package and a direct http
/// tarball dependency both resolve to an `https://….tgz` URL, so the URL alone
/// cannot tell them apart; only the originating spec can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TarballSource {
    /// Registry semver dependency — cache slot `<name>/<version>/`.
    Registry,
    /// Direct `http(s)://…tgz` tarball dependency — cache slot
    /// `<name>/_http_<url_hash>/` (disambiguated by URL so it never collides
    /// with a registry package of the same name/version).
    Http,
    /// Git dependency.
    Git,
    /// Local `file:` tarball or directory dependency.
    File,
}

impl From<&PackageSpec> for TarballSource {
    fn from(spec: &PackageSpec) -> Self {
        match spec {
            PackageSpec::Registry { .. } => Self::Registry,
            PackageSpec::Http { .. } => Self::Http,
            PackageSpec::Git { .. } | PackageSpec::GitHub { .. } => Self::Git,
            PackageSpec::Local { .. } => Self::File,
        }
    }
}

impl From<&str> for PackageSpec {
    fn from(raw: &str) -> Self {
        match Protocol::strip_prefix(raw) {
            Some((Protocol::NpmAlias, rest)) => {
                let (name, version_spec) = parse_package_spec(rest);
                Self::Registry {
                    name: name.to_owned(),
                    version_spec: version_spec.to_owned(),
                }
            }
            Some((Protocol::Git, _)) => {
                let (url, commit_ish) = split_fragment(raw);
                Self::Git {
                    url: url.to_owned(),
                    commit_ish: commit_ish.map(Into::into),
                }
            }
            Some((Protocol::GitHub, rest)) => {
                let (path, commit_ish) = split_fragment(rest);
                if let Some((owner, repo)) = path.split_once('/') {
                    Self::GitHub {
                        owner: owner.to_owned(),
                        repo: repo.to_owned(),
                        commit_ish: commit_ish.map(Into::into),
                    }
                } else {
                    // `github:foo` without `/` — treat as Git URL so it doesn't
                    // silently fall through to the registry resolver.
                    Self::Git {
                        url: raw.to_owned(),
                        commit_ish: commit_ish.map(Into::into),
                    }
                }
            }
            Some((
                proto @ (Protocol::File | Protocol::Link | Protocol::Workspace | Protocol::Portal),
                rest,
            )) => Self::Local {
                protocol: proto,
                path: rest.to_owned(),
            },
            Some((Protocol::Catalog, rest)) => Self::Local {
                protocol: Protocol::Catalog,
                path: rest.to_owned(),
            },
            Some((Protocol::Http, _)) => Self::Http {
                url: raw.to_owned(),
            },
            None => {
                // Bare GitHub shorthand: user/repo or user/repo#ref
                // npm treats "user/repo" (no protocol, not scoped) as github:user/repo
                if !raw.is_scoped() {
                    let (path, commit_ish) = split_fragment(raw);
                    if let Some((owner, repo)) = path.split_once('/')
                        && !owner.is_empty()
                        && !repo.is_empty()
                    {
                        return Self::GitHub {
                            owner: owner.to_owned(),
                            repo: repo.to_owned(),
                            commit_ish: commit_ish.map(Into::into),
                        };
                    }
                }

                // Default: registry spec
                let (name, version_spec) = parse_package_spec(raw);
                Self::Registry {
                    name: name.to_owned(),
                    version_spec: version_spec.to_owned(),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Extension trait
// ---------------------------------------------------------------------------

/// Extension methods for `str` to work with package specs.
///
/// Because [`PackageSpec`] implements [`From<&str>`] (infallible conversion),
/// parsing always succeeds, so we can expose a direct `parse_spec()` that
/// returns `PackageSpec` without `Result`.
///
/// # Examples
/// ```
/// use utoo_ruborist::spec::SpecStr;
///
/// assert!("^1.0.0".is_registry_spec());
/// assert!(!"file:../foo".is_registry_spec());
/// ```
pub trait SpecStr {
    /// Parse into a [`PackageSpec`].  Always succeeds.
    fn parse_spec(&self) -> PackageSpec;
    /// Returns `true` if this is a registry (semver) spec.
    fn is_registry_spec(&self) -> bool;
}

impl SpecStr for str {
    fn parse_spec(&self) -> PackageSpec {
        PackageSpec::from(self)
    }

    fn is_registry_spec(&self) -> bool {
        // Allocation-free mirror of `PackageSpec::from(..).is_registry()` —
        // this runs once per dependency edge on the resolver driver, where the
        // full parse built (and threw away) two `String`s per call. The
        // `spec_str_registry_check_matches_full_parse` test pins equivalence.
        match Protocol::strip_prefix(self) {
            Some((Protocol::NpmAlias, _)) => true,
            Some(_) => false,
            None => {
                // Bare GitHub shorthand (`user/repo`, optionally `#ref`) is the
                // only prefix-less non-registry form.
                if !self.is_scoped() {
                    let (path, _) = split_fragment(self);
                    if let Some((owner, repo)) = path.split_once('/')
                        && !owner.is_empty()
                        && !repo.is_empty()
                    {
                        return false;
                    }
                }
                true
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Catalog protocol
// ---------------------------------------------------------------------------

/// Catalog definitions for the `catalog:` dependency protocol.
///
/// Maps catalog name to (package_name -> version_spec).
/// The default catalog uses key `""` (empty string).
pub type Catalogs = HashMap<String, HashMap<String, String>>;

/// Resolve a `catalog:` spec to its concrete version spec.
///
/// Returns the original spec unchanged if it does not start with `catalog:`.
/// Returns `None` if the catalog or package entry is missing.
pub fn resolve_catalog_spec<'a>(
    pkg_name: &str,
    spec: &'a str,
    catalogs: &'a Catalogs,
) -> Option<&'a str> {
    let catalog_name = spec.strip_prefix("catalog:")?;
    catalogs
        .get(catalog_name)
        .and_then(|c| c.get(pkg_name))
        .map(|s| s.as_str())
}

/// Resolve a `workspace:` spec to the version range that should appear in a
/// **published** manifest, given the concrete `version` of the linked
/// workspace package. Mirrors pnpm / bun pack behavior:
///
/// | Spec                | Result (version = `1.2.3`) |
/// |---------------------|----------------------------|
/// | `workspace:*`       | `1.2.3`                    |
/// | `workspace:~`       | `~1.2.3`                   |
/// | `workspace:^`       | `^1.2.3`                   |
/// | `workspace:^1.2.0`  | `^1.2.0` (prefix stripped) |
/// | `workspace:./path`  | `1.2.3` (path → version)   |
///
/// Returns `None` if `spec` is not a `workspace:` spec.
pub fn resolve_workspace_spec(spec: &str, version: &str) -> Option<String> {
    let rest = spec.strip_prefix("workspace:")?;
    Some(match rest {
        "" | "*" => version.to_string(),
        "~" => format!("~{version}"),
        "^" => format!("^{version}"),
        // Path-based workspace deps (`workspace:./pkg`) publish as the concrete
        // version, since the relative path is meaningless outside the monorepo.
        p if p.starts_with('.') || p.starts_with('/') => version.to_string(),
        // Explicit range/version: strip the prefix and keep it verbatim.
        other => other.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        s.parse_spec()
    }

    #[test]
    fn tarball_source_maps_each_spec_variant() {
        assert_eq!(
            TarballSource::from(&spec("lodash@^4")),
            TarballSource::Registry
        );
        assert_eq!(
            TarballSource::from(&spec("https://example.com/foo.tgz")),
            TarballSource::Http
        );
        assert_eq!(
            TarballSource::from(&spec("git+https://github.com/u/r.git#abc")),
            TarballSource::Git
        );
        assert_eq!(TarballSource::from(&spec("github:u/r")), TarballSource::Git);
        assert_eq!(
            TarballSource::from(&spec("file:../local.tgz")),
            TarballSource::File
        );
    }

    /// The allocation-free `is_registry_spec` must agree with the full parse
    /// for every spec shape the resolver can see.
    #[test]
    fn spec_str_registry_check_matches_full_parse() {
        let cases = [
            "^1.0.0",
            "~2.3.4",
            "*",
            "",
            "latest",
            "1.2.3-beta.1",
            ">=1 <2",
            "npm:lodash@^4.17.0",
            "npm:@scope/pkg",
            "@scope/pkg",
            "@scope/pkg@^1.0.0",
            "workspace:*",
            "workspace:^1.0.0",
            "catalog:",
            "catalog:react",
            "file:../local-pkg",
            "link:../tool",
            "portal:../portal",
            "git+https://github.com/user/repo.git#main",
            "git://github.com/user/repo.git",
            "github:user/repo#v1",
            "github:bare",
            "user/repo",
            "user/repo#branch",
            "https://example.com/pkg.tgz",
            "http://example.com/pkg.tgz",
            "not-a-repo/",
            "/leading-slash",
        ];
        for case in cases {
            assert_eq!(
                case.is_registry_spec(),
                PackageSpec::from(case).is_registry(),
                "is_registry_spec divergence for {case:?}"
            );
        }
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
        assert!(!Protocol::Catalog.is_local());
        assert!(!Protocol::NpmAlias.is_local());
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

    // -- PackageSpec: clone_url --

    #[test]
    fn test_clone_url_strips_git_prefix() {
        let s = spec("git+https://github.com/user/repo.git#main");
        assert_eq!(s.clone_url(), Some("https://github.com/user/repo.git"));
    }

    #[test]
    fn test_clone_url_bare_protocol() {
        // git:// URLs don't have the git+ prefix, so clone_url() returns them as-is
        let s = spec("git://github.com/user/repo.git");
        assert_eq!(s.clone_url(), Some("git://github.com/user/repo.git"));
    }

    #[test]
    fn test_clone_url_non_git_returns_none() {
        let s = spec("lodash@^4.17.0");
        assert_eq!(s.clone_url(), None);
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

    // -- PackageSpec: Http --

    #[test]
    fn test_parse_http_tarball() {
        assert_eq!(
            spec("https://example.com/pkg.tgz"),
            PackageSpec::Http {
                url: "https://example.com/pkg.tgz".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tarball_with_query() {
        assert_eq!(
            spec("https://example.com/pkg.tgz?v=1.0"),
            PackageSpec::Http {
                url: "https://example.com/pkg.tgz?v=1.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_tar_gz() {
        assert_eq!(
            spec("http://example.com/pkg.tar.gz"),
            PackageSpec::Http {
                url: "http://example.com/pkg.tar.gz".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_http_url_without_tarball_extension() {
        // HTTP URLs without .tgz/.tar.gz must NOT fall through to bare GitHub shorthand
        assert_eq!(
            spec("https://example.com/pkg"),
            PackageSpec::Http {
                url: "https://example.com/pkg".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_catalog_spec() {
        // catalog: specs are parsed as Local with Protocol::Catalog
        let s = spec("catalog:default");
        assert_eq!(
            s,
            PackageSpec::Local {
                protocol: Protocol::Catalog,
                path: "default".to_string(),
            }
        );

        let s = spec("catalog:");
        assert_eq!(
            s,
            PackageSpec::Local {
                protocol: Protocol::Catalog,
                path: String::new(),
            }
        );

        let s = spec("catalog:legacy");
        assert_eq!(
            s,
            PackageSpec::Local {
                protocol: Protocol::Catalog,
                path: "legacy".to_string(),
            }
        );
    }

    // -- resolve_workspace_spec --

    #[test]
    fn test_resolve_workspace_spec() {
        let v = "1.2.3";
        assert_eq!(resolve_workspace_spec("workspace:*", v).unwrap(), "1.2.3");
        assert_eq!(resolve_workspace_spec("workspace:~", v).unwrap(), "~1.2.3");
        assert_eq!(resolve_workspace_spec("workspace:^", v).unwrap(), "^1.2.3");
        // Bare `workspace:` behaves like `workspace:*`.
        assert_eq!(resolve_workspace_spec("workspace:", v).unwrap(), "1.2.3");
        // Explicit ranges keep their text, only the prefix is dropped.
        assert_eq!(
            resolve_workspace_spec("workspace:^1.2.0", v).unwrap(),
            "^1.2.0"
        );
        assert_eq!(
            resolve_workspace_spec("workspace:>=1.0.0", v).unwrap(),
            ">=1.0.0"
        );
        // Path-based workspace deps publish as the concrete version.
        assert_eq!(
            resolve_workspace_spec("workspace:./pkgs/foo", v).unwrap(),
            "1.2.3"
        );
        // Non-workspace specs are left untouched.
        assert!(resolve_workspace_spec("^1.0.0", v).is_none());
        assert!(resolve_workspace_spec("catalog:", v).is_none());
    }

    // -- PackageSpec: npm alias --

    #[test]
    fn test_parse_npm_alias() {
        assert_eq!(
            spec("npm:lodash@^4.17.0"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "^4.17.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_npm_alias_scoped() {
        assert_eq!(
            spec("npm:@scope/pkg@^1.0.0"),
            PackageSpec::Registry {
                name: "@scope/pkg".to_string(),
                version_spec: "^1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_npm_alias_no_version() {
        assert_eq!(
            spec("npm:lodash"),
            PackageSpec::Registry {
                name: "lodash".to_string(),
                version_spec: "*".to_string(),
            }
        );
    }
}
