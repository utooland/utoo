//! Semver matching utilities using deno_semver.

use std::borrow::Cow;

use deno_semver::{Version, VersionReq};

/// Normalize spec for registry requests.
///
/// Handles special prefixes:
/// - `npm:package@version` -> (package, version) - alias to different package
/// - `workspace:*` -> (original_name, *) - workspace reference
///
/// Returns (normalized_name, normalized_spec).
///
/// # Examples
/// ```
/// use utoo_ruborist::resolver::semver::normalize_spec;
///
/// // npm alias
/// let (name, spec) = normalize_spec("string-width-cjs", "npm:string-width@^4.2.0");
/// assert_eq!(name, "string-width");
/// assert_eq!(spec, "^4.2.0");
///
/// // npm alias without version
/// let (name, spec) = normalize_spec("lodash-cjs", "npm:lodash");
/// assert_eq!(name, "lodash");
/// assert_eq!(spec, "*");
///
/// // scoped npm alias
/// let (name, spec) = normalize_spec("my-pkg", "npm:@scope/pkg@^1.0.0");
/// assert_eq!(name, "@scope/pkg");
/// assert_eq!(spec, "^1.0.0");
///
/// // workspace reference
/// let (name, spec) = normalize_spec("my-lib", "workspace:*");
/// assert_eq!(name, "my-lib");
/// assert_eq!(spec, "*");
///
/// // regular spec (unchanged)
/// let (name, spec) = normalize_spec("lodash", "^4.0.0");
/// assert_eq!(name, "lodash");
/// assert_eq!(spec, "^4.0.0");
/// ```
pub fn normalize_spec<'a>(name: &'a str, spec: &'a str) -> (Cow<'a, str>, Cow<'a, str>) {
    // Handle npm: alias - fetch the aliased package instead
    if let Some(npm_spec) = spec.strip_prefix("npm:") {
        let (pkg_name, version) = split_npm_alias(npm_spec);
        return (Cow::Borrowed(pkg_name), Cow::Borrowed(version));
    }

    // Handle workspace: prefix - keep original name, extract version
    if let Some(workspace_spec) = spec.strip_prefix("workspace:") {
        // Skip "workspace:"
        return (Cow::Borrowed(name), Cow::Borrowed(workspace_spec));
    }

    // No special prefix, return as-is — borrowed, so the per-edge common case
    // (plain registry spec) allocates nothing.
    (Cow::Borrowed(name), Cow::Borrowed(spec))
}

/// Split an `npm:` alias body (after the prefix) into (aliased name, range).
///
/// `rfind('@')` handles scoped packages (`@scope/pkg@^1.0.0`); an `@` at
/// index 0 is the scope marker of a version-less alias (`@scope/pkg`), not a
/// separator.
fn split_npm_alias(npm_spec: &str) -> (&str, &str) {
    match npm_spec.rfind('@') {
        Some(idx) if idx > 0 => (&npm_spec[..idx], &npm_spec[idx + 1..]),
        _ => (npm_spec, "*"),
    }
}

/// Check if a version matches a semver range.
///
/// Handles special cases:
/// - `npm:` prefix (alias packages)
/// - `*` (matches any version)
/// - dist-tags (always match)
///
/// # Examples
/// ```
/// use utoo_ruborist::resolver::semver::matches;
///
/// assert!(matches("^1.0.0", "1.2.3"));
/// assert!(!matches("^2.0.0", "1.2.3"));
/// assert!(matches("*", "1.2.3"));
/// ```
pub fn matches(range: &str, version: &str) -> bool {
    // Handle npm: alias prefix via the shared splitter so scoped aliases
    // (`npm:@scope/pkg`) don't get sliced at the scope marker.
    let range = match range.strip_prefix("npm:") {
        Some(npm_spec) => split_npm_alias(npm_spec).1,
        None => range,
    };

    // Wildcard matches everything
    if range == "*" {
        return true;
    }

    let req = match VersionReq::parse_from_npm(range) {
        Ok(req) => req,
        Err(_) => return false,
    };

    // Dist-tags (like "latest", "beta") always match
    if req.tag().is_some() {
        return true;
    }

    let version = match Version::parse_from_npm(version) {
        Ok(v) => v,
        Err(_) => return false,
    };

    req.matches(&version)
}

/// Find the maximum version from a list that satisfies a semver range.
///
/// # Examples
/// ```
/// use utoo_ruborist::resolver::semver::max_satisfying;
///
/// let versions = ["1.0.0", "1.1.0", "1.2.0", "2.0.0"];
/// let result = max_satisfying(versions.iter().copied(), "^1.0.0");
/// assert_eq!(result.map(|v| v.to_string()), Some("1.2.0".to_string()));
/// ```
pub fn max_satisfying<'a>(versions: impl Iterator<Item = &'a str>, range: &str) -> Option<Version> {
    let req = VersionReq::parse_from_npm(range).ok()?;

    match range {
        "*" => versions
            .filter_map(|v| Version::parse_from_npm(v).ok())
            .max(),
        _ => versions
            .filter_map(|v| Version::parse_from_npm(v).ok())
            .filter(|v| req.matches(v))
            .max(),
    }
}

/// [`max_satisfying`] over a pre-parsed, descending-sorted list: the first
/// match is the maximum, so this early-exits instead of parsing and scanning
/// every version per spec. Pair with the per-package memoized sort
/// (`FullManifest::sorted_parsed_versions`).
pub fn max_satisfying_sorted_desc<'a>(sorted: &'a [Version], range: &str) -> Option<&'a Version> {
    let req = VersionReq::parse_from_npm(range).ok()?;
    if range == "*" {
        return sorted.first();
    }
    sorted.iter().find(|v| req.matches(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_matching() {
        // Exact version
        assert!(matches("1.2.3", "1.2.3"));
        assert!(!matches("1.2.3", "1.2.4"));

        // Caret range
        assert!(matches("^1.2.3", "1.3.0"));
        assert!(!matches("^1.2.3", "2.0.0"));

        // Tilde range
        assert!(matches("~1.2.3", "1.2.9"));
        assert!(!matches("~1.2.3", "1.3.0"));

        // Wildcard
        assert!(matches("*", "1.2.3"));

        // Dist-tag (always matches)
        assert!(matches("beta", "1.2.3"));

        // Invalid version
        assert!(!matches("1.2.3", "invalid"));
    }

    #[test]
    fn test_npm_alias() {
        assert!(matches("npm:lodash@^4.0.0", "4.17.21"));
        assert!(!matches("npm:lodash@^4.0.0", "3.10.0"));
        assert!(matches("npm:lodash", "4.17.21")); // No version = *
    }

    #[test]
    fn test_scoped_npm_alias() {
        assert!(matches("npm:@scope/pkg@^1.0.0", "1.2.3"));
        assert!(!matches("npm:@scope/pkg@^2.0.0", "1.2.3"));
        // Version-less scoped alias: the scope `@` is not a separator => *
        assert!(matches("npm:@scope/pkg", "1.2.3"));
    }

    #[test]
    fn test_max_satisfying() {
        let versions = ["1.0.0", "1.1.0", "1.2.0", "2.0.0", "2.1.0"];

        assert_eq!(
            max_satisfying(versions.iter().copied(), "^1.0.0"),
            Some(Version::parse_from_npm("1.2.0").unwrap())
        );

        assert_eq!(
            max_satisfying(versions.iter().copied(), "^2.0.0"),
            Some(Version::parse_from_npm("2.1.0").unwrap())
        );

        assert_eq!(
            max_satisfying(versions.iter().copied(), "~1.1.0"),
            Some(Version::parse_from_npm("1.1.0").unwrap())
        );

        assert_eq!(
            max_satisfying(versions.iter().copied(), ">=2.0.0"),
            Some(Version::parse_from_npm("2.1.0").unwrap())
        );

        assert_eq!(max_satisfying(versions.iter().copied(), "^3.0.0"), None);

        assert_eq!(
            max_satisfying(versions.iter().copied(), "^1"),
            Some(Version::parse_from_npm("1.2.0").unwrap())
        );
    }

    #[test]
    fn test_normalize_spec() {
        // npm alias
        assert_eq!(
            normalize_spec("string-width-cjs", "npm:string-width@^4.2.0"),
            (Cow::Borrowed("string-width"), Cow::Borrowed("^4.2.0"))
        );

        // npm alias without version
        assert_eq!(
            normalize_spec("lodash-cjs", "npm:lodash"),
            (Cow::Borrowed("lodash"), Cow::Borrowed("*"))
        );

        // scoped npm alias
        assert_eq!(
            normalize_spec("my-pkg", "npm:@scope/pkg@^1.0.0"),
            (Cow::Borrowed("@scope/pkg"), Cow::Borrowed("^1.0.0"))
        );

        // scoped npm alias without version
        assert_eq!(
            normalize_spec("my-pkg", "npm:@scope/pkg"),
            (Cow::Borrowed("@scope/pkg"), Cow::Borrowed("*"))
        );

        // workspace reference
        assert_eq!(
            normalize_spec("my-lib", "workspace:*"),
            (Cow::Borrowed("my-lib"), Cow::Borrowed("*"))
        );

        assert_eq!(
            normalize_spec("my-lib", "workspace:^1.0.0"),
            (Cow::Borrowed("my-lib"), Cow::Borrowed("^1.0.0"))
        );

        // regular spec (unchanged)
        assert_eq!(
            normalize_spec("lodash", "^4.0.0"),
            (Cow::Borrowed("lodash"), Cow::Borrowed("^4.0.0"))
        );

        // scoped package regular spec
        assert_eq!(
            normalize_spec("@types/node", "^18.0.0"),
            (Cow::Borrowed("@types/node"), Cow::Borrowed("^18.0.0"))
        );
    }
}
