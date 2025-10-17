use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::path::Path;

/// Read and parse a JSON file into the specified type
pub async fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read file {}", path.display()))?;

    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

/// Read and parse a JSON file into a serde_json::Value
pub async fn read_json_value(path: &Path) -> Result<serde_json::Value> {
    read_json_file(path).await
}

/// Load package.json from specified path
pub async fn load_package_json_from_path(path: &Path) -> Result<Value> {
    read_json_value(&path.join("package.json")).await
}

pub async fn load_package_lock_json_from_path(path: &Path) -> Result<Value> {
    read_json_value(&path.join("package-lock.json")).await
}

/// Merge two Option<&Map<String, Value>> into a new Map, left has higher priority
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
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_read_json_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.json");

        // Create a test JSON file
        let test_data = json!({
            "name": "test",
            "version": "1.0.0",
            "dependencies": {
                "dep1": "^1.0.0"
            }
        });

        fs::write(&file_path, test_data.to_string()).unwrap();

        // Test reading into Value
        let value: Value = read_json_file(&file_path).await.unwrap();
        assert_eq!(value["name"], "test");
        assert_eq!(value["version"], "1.0.0");

        // Test reading into custom type
        #[derive(serde::Deserialize)]
        struct TestPackage {
            name: String,
            version: String,
        }

        let package: TestPackage = read_json_file(&file_path).await.unwrap();
        assert_eq!(package.name, "test");
        assert_eq!(package.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_read_json_value() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.json");

        let test_data = json!({
            "key": "value",
            "number": 42
        });

        fs::write(&file_path, test_data.to_string()).unwrap();

        let value = read_json_value(&file_path).await.unwrap();
        assert_eq!(value["key"], "value");
        assert_eq!(value["number"], 42);
    }

    #[tokio::test]
    async fn test_load_package_json_from_path() {
        let dir = tempdir().unwrap();
        let package_path = dir.path().join("package.json");

        let test_data = json!({
            "name": "test-package",
            "version": "1.0.0"
        });

        fs::write(&package_path, test_data.to_string()).unwrap();

        let value = load_package_json_from_path(dir.path()).await.unwrap();
        assert_eq!(value["name"], "test-package");
        assert_eq!(value["version"], "1.0.0");
    }

    #[tokio::test]
    async fn test_error_handling() {
        let non_existent_path = Path::new("non_existent.json");

        // Test error handling for non-existent file
        let result: Result<Value> = read_json_file(non_existent_path).await;
        assert!(result.is_err());

        // Test error handling for invalid JSON
        let dir = tempdir().unwrap();
        let invalid_json_path = dir.path().join("invalid.json");
        fs::write(&invalid_json_path, "invalid json content").unwrap();

        let result: Result<Value> = read_json_file(&invalid_json_path).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_json_objects() {
        use serde_json::Map;
        use serde_json::json;

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

        // Both, different keys
        let mut left_map = Map::new();
        left_map.insert("x".to_string(), json!(1));
        let mut right_map = Map::new();
        right_map.insert("y".to_string(), json!(2));
        let merged = merge_json_objects(Some(&left_map), Some(&right_map));
        assert_eq!(merged.len(), 2);
        assert_eq!(merged["x"], json!(1));
        assert_eq!(merged["y"], json!(2));
    }
}
