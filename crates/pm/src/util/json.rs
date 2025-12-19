use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::Path;

/// Read and parse a JSON file into the specified type
pub async fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let content = crate::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read file {}", path.display()))?;

    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

/// Load package.json from specified path
pub async fn load_package_json_from_path(path: &Path) -> Result<Value> {
    read_json_file(&path.join("package.json")).await
}

pub async fn load_package_lock_json_from_path(path: &Path) -> Result<Value> {
    read_json_file(&path.join("package-lock.json")).await
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
}
