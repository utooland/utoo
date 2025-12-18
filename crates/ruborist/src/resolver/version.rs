//! Version resolution from dist-tags and version lists.

use std::collections::HashMap;

use super::semver::{matches, max_satisfying};

/// Resolve target version from dist-tags and version list.
///
/// This is the core version resolution logic that matches npm's behavior:
/// 1. If spec is a dist-tag, return the tagged version
/// 2. If 'latest' dist-tag satisfies the spec, prefer it (npm behavior)
/// 3. Otherwise, find the maximum satisfying version
///
/// # Arguments
/// * `dist_tags` - Map of tag names to versions (e.g., {"latest": "1.2.3"})
/// * `version_list` - List of all available versions
/// * `spec` - Version specification to resolve
///
/// # Examples
/// ```
/// use std::collections::HashMap;
/// use utoo_ruborist::resolver::version::resolve_target_version;
///
/// let mut dist_tags = HashMap::new();
/// dist_tags.insert("latest".to_string(), "1.2.3".to_string());
///
/// let versions = vec!["1.0.0".to_string(), "1.2.3".to_string(), "1.5.0".to_string()];
///
/// // Prefer latest when it satisfies the spec
/// let result = resolve_target_version(&dist_tags, &versions, "^1.0.0");
/// assert_eq!(result, Ok("1.2.3".to_string()));
/// ```
pub fn resolve_target_version(
    dist_tags: &HashMap<String, String>,
    version_list: &[String],
    spec: &str,
) -> Result<String, ResolveError> {
    if version_list.is_empty() {
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
        .map(|latest| {
            tracing::debug!("Using dist-tags 'latest' version {latest} for spec {spec}");
            latest.to_string()
        })
        .or_else(|| {
            max_satisfying(version_list.iter().map(|s| s.as_str()), spec).map(|v| v.to_string())
        });

    version.ok_or_else(|| ResolveError::NoMatchingVersion {
        spec: spec.to_string(),
        available_count: version_list.len(),
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
    use super::*;

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
            resolve_target_version(&dist_tags, &versions, "latest"),
            Ok("1.2.3".to_string())
        );
        assert_eq!(
            resolve_target_version(&dist_tags, &versions, "beta"),
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
            resolve_target_version(&dist_tags, &versions, "^1.0.0"),
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
            resolve_target_version(&dist_tags, &versions, "^2.0.0"),
            Ok("2.1.0".to_string())
        );
    }

    #[test]
    fn test_resolve_no_match() {
        let dist_tags = HashMap::new();
        let versions = vec!["1.0.0".to_string()];

        let result = resolve_target_version(&dist_tags, &versions, "^2.0.0");
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

        let result = resolve_target_version(&dist_tags, &versions, "^1.0.0");
        assert_eq!(result, Err(ResolveError::NoVersionsAvailable));
    }
}
