//! Semver matching utilities using deno_semver.

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
pub fn normalize_spec(name: &str, spec: &str) -> (String, String) {
    // Handle npm: alias - fetch the aliased package instead
    if let Some(npm_spec) = spec.strip_prefix("npm:") {
        // Skip "npm:"
        // Use rfind to handle scoped packages like @scope/pkg@version
        if let Some(last_at_index) = npm_spec.rfind('@') {
            // Make sure we don't split on the @ of a scoped package
            if last_at_index > 0 {
                let (pkg_name, version) = npm_spec.split_at(last_at_index);
                return (pkg_name.to_string(), version[1..].to_string());
            }
        }
        // No version specified
        return (npm_spec.to_string(), "*".to_string());
    }

    // Handle workspace: prefix - keep original name, extract version
    if let Some(workspace_spec) = spec.strip_prefix("workspace:") {
        // Skip "workspace:"
        return (name.to_string(), workspace_spec.to_string());
    }

    // No special prefix, return as-is
    (name.to_string(), spec.to_string())
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
    // Handle npm: alias prefix
    let range = if range.starts_with("npm:") {
        if let Some(idx) = range.rfind('@') {
            &range[idx + 1..]
        } else {
            "*"
        }
    } else {
        range
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
            ("string-width".to_string(), "^4.2.0".to_string())
        );

        // npm alias without version
        assert_eq!(
            normalize_spec("lodash-cjs", "npm:lodash"),
            ("lodash".to_string(), "*".to_string())
        );

        // scoped npm alias
        assert_eq!(
            normalize_spec("my-pkg", "npm:@scope/pkg@^1.0.0"),
            ("@scope/pkg".to_string(), "^1.0.0".to_string())
        );

        // scoped npm alias without version
        assert_eq!(
            normalize_spec("my-pkg", "npm:@scope/pkg"),
            ("@scope/pkg".to_string(), "*".to_string())
        );

        // workspace reference
        assert_eq!(
            normalize_spec("my-lib", "workspace:*"),
            ("my-lib".to_string(), "*".to_string())
        );

        assert_eq!(
            normalize_spec("my-lib", "workspace:^1.0.0"),
            ("my-lib".to_string(), "^1.0.0".to_string())
        );

        // regular spec (unchanged)
        assert_eq!(
            normalize_spec("lodash", "^4.0.0"),
            ("lodash".to_string(), "^4.0.0".to_string())
        );

        // scoped package regular spec
        assert_eq!(
            normalize_spec("@types/node", "^18.0.0"),
            ("@types/node".to_string(), "^18.0.0".to_string())
        );
    }
}
