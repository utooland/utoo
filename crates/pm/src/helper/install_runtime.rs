use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

// {engines: {install-node: "14.17.0"}}
// will return optionalDependencies HashMap
//   "optionalDependencies": {
//     "node-bin-darwin-arm64": "16",
//     "node-darwin-x64": "16",
//     "node-linux-arm64": "16",
//     "node-linux-x64": "16",
//     "node-win-x64": "16",
//     "node-win-x86": "16"
//   }

// node-bin-darwin-arm64 is not supported for node@<16
// node-darwin-arm64 is not supported for node@>=16
// since node@14 is no longer maintained, we do not use node-darwin-arm64

// Platform to supported architectures mapping
const PLATFORM_ARCHS: &[(&str, &str, &[&str])] = &[
    ("node", "darwin", &["x64"]),
    ("node-bin", "darwin", &["arm64"]),
    ("node", "linux", &["x64", "arm64"]),
    ("node", "win", &["x64", "x86"]),
];

pub fn install_runtime(engines: &Value) -> Result<HashMap<String, String>> {
    // Get node version from engines.install-node or use default
    let version = engines
        .get("install-node")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if version.is_empty() {
        return Ok(HashMap::new());
    }

    get_node_deps(version)
}

fn get_node_deps(version: &str) -> Result<HashMap<String, String>> {
    let mut optional_deps = HashMap::new();

    // Iterate through platform and their supported architectures
    for (prefix, platform, archs) in PLATFORM_ARCHS {
        for arch in *archs {
            let dep_name: String = format!("{prefix}-{platform}-{arch}");
            optional_deps.insert(dep_name, version.to_string());
        }
    }
    Ok(optional_deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_install_runtime_with_empty_version() {
        let engines = json!({
            "install-node": ""
        });

        let result = install_runtime(&engines).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_install_runtime_with_valid_version() {
        let engines = json!({
            "install-node": "16.17.0"
        });

        let result = install_runtime(&engines).unwrap();

        // Check if all expected dependencies are present
        assert_eq!(result.get("node-darwin-x64"), Some(&"16.17.0".to_string()));
        assert_eq!(
            result.get("node-bin-darwin-arm64"),
            Some(&"16.17.0".to_string())
        );
        assert_eq!(result.get("node-linux-x64"), Some(&"16.17.0".to_string()));
        assert_eq!(result.get("node-linux-arm64"), Some(&"16.17.0".to_string()));
        assert_eq!(result.get("node-win-x64"), Some(&"16.17.0".to_string()));
        assert_eq!(result.get("node-win-x86"), Some(&"16.17.0".to_string()));

        // Check total number of dependencies
        assert_eq!(result.len(), 6);
    }

    #[test]
    fn test_install_runtime_with_no_version() {
        let engines = json!({
            "install-node": ""
        });

        let result = install_runtime(&engines).unwrap();
        // Check if all expected dependencies are present
        // Check total number of dependencies
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_install_runtime_with_no_config() {
        let engines = json!({});

        let result = install_runtime(&engines).unwrap();
        // Check if all expected dependencies are present
        // Check total number of dependencies
        assert_eq!(result.len(), 0);
    }
}
