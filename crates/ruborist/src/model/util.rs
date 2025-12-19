//! Internal utility functions for model parsing.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::package_json::PackageJson;

/// Read and parse package.json from a directory.
pub async fn read_package_json(dir: &Path) -> Result<PackageJson> {
    let path = dir.join("package.json");
    let bytes = tokio_fs_ext::read(&path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_slice(&bytes).context(format!("Failed to parse {}", path.display()))
}

/// Parse a package spec string into (name, version_spec).
///
/// # Examples
/// - `"lodash"` -> `("lodash", "*")`
/// - `"lodash@^4.17.0"` -> `("lodash", "^4.17.0")`
/// - `"@scope/pkg@1.0.0"` -> `("@scope/pkg", "1.0.0")`
pub fn parse_package_spec(spec: &str) -> (&str, &str) {
    if let Some(stripped) = spec.strip_prefix('@') {
        // Scoped package: @scope/name@version
        if let Some(idx) = stripped.find('@') {
            let idx = idx + 1; // Account for the stripped '@'
            (&spec[..idx], &spec[idx + 1..])
        } else {
            (spec, "*")
        }
    } else {
        // Regular package: name@version
        spec.rfind('@')
            .map_or((spec, "*"), |pos| (&spec[..pos], &spec[pos + 1..]))
    }
}

/// Merge two optional JSON objects, left has higher priority.
pub fn merge_json_objects(
    left: Option<&Map<String, Value>>,
    right: Option<&Map<String, Value>>,
) -> Map<String, Value> {
    let mut merged = Map::new();
    if let Some(r) = right {
        for (k, v) in r {
            merged.insert(k.clone(), v.clone());
        }
    }
    if let Some(l) = left {
        for (k, v) in l {
            merged.insert(k.clone(), v.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_package_spec() {
        assert_eq!(parse_package_spec("lodash"), ("lodash", "*"));
        assert_eq!(parse_package_spec("lodash@^4.17.0"), ("lodash", "^4.17.0"));
        assert_eq!(parse_package_spec("@scope/pkg"), ("@scope/pkg", "*"));
        assert_eq!(
            parse_package_spec("@scope/pkg@1.0.0"),
            ("@scope/pkg", "1.0.0")
        );
        assert_eq!(
            parse_package_spec("@types/node@^18.0.0"),
            ("@types/node", "^18.0.0")
        );
    }

    #[test]
    fn test_merge_json_objects() {
        // Both None
        let merged = merge_json_objects(None, None);
        assert!(merged.is_empty());

        // Only left
        let mut left_map = Map::new();
        left_map.insert("a".to_string(), json!(1));
        let merged = merge_json_objects(Some(&left_map), None);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged["a"], json!(1));

        // Only right
        let mut right_map = Map::new();
        right_map.insert("b".to_string(), json!(2));
        let merged = merge_json_objects(None, Some(&right_map));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged["b"], json!(2));

        // Both, left has priority
        let mut left_map = Map::new();
        left_map.insert("k".to_string(), json!(100));
        let mut right_map = Map::new();
        right_map.insert("k".to_string(), json!(200));
        let merged = merge_json_objects(Some(&left_map), Some(&right_map));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged["k"], json!(100));
    }
}
