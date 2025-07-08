use anyhow::{Result, anyhow};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fs::File;
use std::path::Path;

/// Read and parse a JSON file into the specified type
pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file =
        File::open(path).map_err(|e| anyhow!("Failed to open file {}: {}", path.display(), e))?;

    serde_json::from_reader(file)
        .map_err(|e| anyhow!("Failed to parse JSON from {}: {}", path.display(), e))
}

/// Read and parse a JSON file into a serde_json::Value
pub fn read_json_value(path: &Path) -> Result<serde_json::Value> {
    read_json_file(path)
}

/// Load package.json from specified path
pub fn load_package_json_from_path(path: &Path) -> Result<Value> {
    read_json_value(&path.join("package.json"))
}

pub fn load_package_lock_json_from_path(path: &Path) -> Result<Value> {
    read_json_value(&path.join("package-lock.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_read_json_file() {
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
        let value: Value = read_json_file(&file_path).unwrap();
        assert_eq!(value["name"], "test");
        assert_eq!(value["version"], "1.0.0");

        // Test reading into custom type
        #[derive(serde::Deserialize)]
        struct TestPackage {
            name: String,
            version: String,
        }

        let package: TestPackage = read_json_file(&file_path).unwrap();
        assert_eq!(package.name, "test");
        assert_eq!(package.version, "1.0.0");
    }

    #[test]
    fn test_read_json_value() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.json");

        let test_data = json!({
            "key": "value",
            "number": 42
        });

        fs::write(&file_path, test_data.to_string()).unwrap();

        let value = read_json_value(&file_path).unwrap();
        assert_eq!(value["key"], "value");
        assert_eq!(value["number"], 42);
    }

    #[test]
    fn test_load_package_json_from_path() {
        let dir = tempdir().unwrap();
        let package_path = dir.path().join("package.json");

        let test_data = json!({
            "name": "test-package",
            "version": "1.0.0"
        });

        fs::write(&package_path, test_data.to_string()).unwrap();

        let value = load_package_json_from_path(dir.path()).unwrap();
        assert_eq!(value["name"], "test-package");
        assert_eq!(value["version"], "1.0.0");
    }

    #[test]
    fn test_error_handling() {
        let non_existent_path = Path::new("non_existent.json");

        // Test error handling for non-existent file
        let result: Result<Value> = read_json_file(non_existent_path);
        assert!(result.is_err());

        // Test error handling for invalid JSON
        let dir = tempdir().unwrap();
        let invalid_json_path = dir.path().join("invalid.json");
        fs::write(&invalid_json_path, "invalid json content").unwrap();

        let result: Result<Value> = read_json_file(&invalid_json_path);
        assert!(result.is_err());
    }
}
