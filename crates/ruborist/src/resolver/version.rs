//! Version resolution from dist-tags and version lists.

use crate::model::manifest::VersionsRef;

use super::semver::{matches, max_satisfying};

/// Resolve target version from a [`VersionsRef`] view.
///
/// This is the core version resolution logic that matches npm's behavior:
/// 1. If spec is a dist-tag, return the tagged version
/// 2. If 'latest' dist-tag satisfies the spec, prefer it (npm behavior)
/// 3. Otherwise, find the maximum satisfying version
///
/// # Arguments
/// * `view` - Borrowed view of the versions list + dist-tag map. Construct via
///   `VersionsRef::from(&full_manifest)` or `VersionsRef::from(&versions_info)`.
/// * `spec` - Version specification to resolve
///
/// # Examples
/// ```
/// use std::collections::HashMap;
/// use utoo_ruborist::manifest::VersionsRef;
/// use utoo_ruborist::resolver::version::resolve_target_version;
///
/// let mut dist_tags = HashMap::new();
/// dist_tags.insert("latest".to_string(), "1.2.3".to_string());
///
/// let versions = vec!["1.0.0".to_string(), "1.2.3".to_string(), "1.5.0".to_string()];
///
/// // Prefer latest when it satisfies the spec
/// let view = VersionsRef { versions: &versions, dist_tags: &dist_tags };
/// let result = resolve_target_version(view, "^1.0.0");
/// assert_eq!(result, Ok("1.2.3".to_string()));
/// ```
pub fn resolve_target_version(view: VersionsRef<'_>, spec: &str) -> Result<String, ResolveError> {
    let VersionsRef {
        versions,
        dist_tags,
    } = view;

    if versions.is_empty() {
        return Err(ResolveError::NoVersionsAvailable);
    }

    // First check if spec is a dist-tag
    if let Some(version) = dist_tags.get(spec) {
        return Ok(version.to_string());
    }

    // Not a dist-tag, do semver matching
    // Check if 'latest' dist-tag satisfies the spec (npm behavior)
    let version = dist_tags
        .get("latest")
        .filter(|latest| matches(spec, latest))
        .map(|latest| latest.to_string())
        .or_else(|| {
            max_satisfying(versions.iter().map(|s| s.as_str()), spec).map(|v| v.to_string())
        });

    version.ok_or_else(|| ResolveError::NoMatchingVersion {
        spec: spec.to_string(),
        available_count: versions.len(),
    })
}

/// [`resolve_target_version`] backed by a per-package pre-parsed,
/// descending-sorted version list (see `FullManifest::sorted_parsed_versions`):
/// identical dist-tag / latest-preference policy, but the semver fallback
/// early-exits on the first (= maximum) match instead of parsing every version
/// per spec.
///
/// `sorted` is a closure on purpose: the sorted list is a lazily-built
/// per-package cache whose first build parses the whole version list — an
/// eager `&[Version]` argument would force that build even on the dist-tag /
/// latest shortcuts that never look at it (and on the resolver driver thread,
/// at that). The closure only runs when the fallback is actually reached.
pub fn resolve_target_version_lazy<'a>(
    view: VersionsRef<'_>,
    sorted: impl FnOnce() -> &'a [deno_semver::Version],
    spec: &str,
) -> Result<String, ResolveError> {
    let VersionsRef {
        versions,
        dist_tags,
    } = view;

    if versions.is_empty() {
        return Err(ResolveError::NoVersionsAvailable);
    }

    if let Some(version) = dist_tags.get(spec) {
        return Ok(version.to_string());
    }

    let version = dist_tags
        .get("latest")
        .filter(|latest| matches(spec, latest))
        .map(|latest| latest.to_string())
        .or_else(|| {
            super::semver::max_satisfying_sorted_desc(sorted(), spec).map(|v| v.to_string())
        });

    version.ok_or_else(|| ResolveError::NoMatchingVersion {
        spec: spec.to_string(),
        available_count: versions.len(),
    })
}

/// Error type for version resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No versions available for the package
    NoVersionsAvailable,
    /// No version matches the given spec
    NoMatchingVersion {
        spec: String,
        available_count: usize,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NoVersionsAvailable => write!(f, "No versions available"),
            ResolveError::NoMatchingVersion {
                spec,
                available_count,
            } => write!(
                f,
                "No matching version found for spec '{}' from {} available versions",
                spec, available_count
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn view<'a>(versions: &'a [String], dist_tags: &'a HashMap<String, String>) -> VersionsRef<'a> {
        VersionsRef {
            versions,
            dist_tags,
        }
    }

    #[test]
    fn test_resolve_dist_tag() {
        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.2.3".to_string());
        dist_tags.insert("beta".to_string(), "2.0.0-beta.1".to_string());

        let versions = vec![
            "1.0.0".to_string(),
            "1.2.3".to_string(),
            "2.0.0-beta.1".to_string(),
        ];

        // Resolve dist-tag directly
        assert_eq!(
            resolve_target_version(view(&versions, &dist_tags), "latest"),
            Ok("1.2.3".to_string())
        );
        assert_eq!(
            resolve_target_version(view(&versions, &dist_tags), "beta"),
            Ok("2.0.0-beta.1".to_string())
        );
    }

    #[test]
    fn test_resolve_prefer_latest() {
        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.5.0".to_string());

        let versions = vec![
            "1.0.0".to_string(),
            "1.5.0".to_string(),
            "1.9.0".to_string(),
        ];

        // Should prefer latest (1.5.0) over max_satisfying (1.9.0)
        assert_eq!(
            resolve_target_version(view(&versions, &dist_tags), "^1.0.0"),
            Ok("1.5.0".to_string())
        );
    }

    #[test]
    fn test_resolve_fallback_to_max() {
        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.5.0".to_string());

        let versions = vec![
            "1.5.0".to_string(),
            "2.0.0".to_string(),
            "2.1.0".to_string(),
        ];

        // latest doesn't satisfy ^2.0.0, should use max_satisfying (2.1.0)
        assert_eq!(
            resolve_target_version(view(&versions, &dist_tags), "^2.0.0"),
            Ok("2.1.0".to_string())
        );
    }

    #[test]
    fn test_resolve_no_match() {
        let dist_tags = HashMap::new();
        let versions = vec!["1.0.0".to_string()];

        let result = resolve_target_version(view(&versions, &dist_tags), "^2.0.0");
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ResolveError::NoMatchingVersion { .. })
        ));
    }

    #[test]
    fn test_resolve_empty_versions() {
        let dist_tags = HashMap::new();
        let versions: Vec<String> = vec![];

        let result = resolve_target_version(view(&versions, &dist_tags), "^1.0.0");
        assert_eq!(result, Err(ResolveError::NoVersionsAvailable));
    }

    /// The lazy variant must not build the sorted list when the dist-tag or
    /// latest shortcut settles the spec — that build is the expensive
    /// per-package parse+sort the shortcuts exist to avoid.
    #[test]
    fn test_lazy_skips_sorted_on_shortcut() {
        use std::cell::Cell;

        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.5.0".to_string());
        dist_tags.insert("beta".to_string(), "2.0.0-beta.1".to_string());
        let versions = vec!["1.0.0".to_string(), "1.5.0".to_string()];

        let called = Cell::new(false);
        let sorted: Vec<deno_semver::Version> = vec![];

        // dist-tag hit: closure untouched
        let result = resolve_target_version_lazy(
            view(&versions, &dist_tags),
            || {
                called.set(true);
                &sorted
            },
            "beta",
        );
        assert_eq!(result, Ok("2.0.0-beta.1".to_string()));
        assert!(
            !called.get(),
            "dist-tag shortcut must not build sorted list"
        );

        // latest satisfies: closure untouched
        let result = resolve_target_version_lazy(
            view(&versions, &dist_tags),
            || {
                called.set(true);
                &sorted
            },
            "^1.0.0",
        );
        assert_eq!(result, Ok("1.5.0".to_string()));
        assert!(!called.get(), "latest shortcut must not build sorted list");

        // latest misses: closure runs, early-exit picks the max match
        let parsed = crate::model::manifest::sort_parsed_versions(&[
            "1.0.0".to_string(),
            "0.9.0".to_string(),
        ]);
        let result = resolve_target_version_lazy(
            view(&versions, &dist_tags),
            || {
                called.set(true);
                &parsed
            },
            "^0.9.0",
        );
        assert_eq!(result, Ok("0.9.0".to_string()));
        assert!(called.get(), "fallback must consult the sorted list");
    }
}
