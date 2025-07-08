use crate::util::config::get_registry;
use crate::util::json::load_package_json_from_path;
use crate::util::logger::log_info;
use crate::util::semver::matches;
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Map, Value};
use std::path::Path;
use tokio::fs;
use tokio::sync::OnceCell;

static CONFIG: OnceCell<Value> = OnceCell::const_new();

async fn load_config() -> Result<&'static Value> {
    CONFIG
        .get_or_try_init(|| async {
            let registry = get_registry();
            let url = format!("{registry}/binary-mirror-config/latest");
            let response = reqwest::get(&url)
                .await
                .context("Failed to fetch binary mirror config")?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!("HTTP status: {}", response.status()));
            }

            response
                .json()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to parse binary mirror config: {}", e))
        })
        .await
}

fn update_binary_config(pkg: &mut Value, binary_mirror: &Map<String, Value>) {
    // Get existing binary configuration
    let mut new_binary = if let Some(binary) = pkg.get("binary") {
        if let Some(obj) = binary.as_object() {
            obj.clone()
        } else {
            Map::new()
        }
    } else {
        Map::new()
    };

    // Merge new configuration
    for (key, value) in binary_mirror {
        if key != "replaceHostFiles" {
            new_binary.insert(key.clone(), value.clone());
        }
    }

    // Update binary configuration
    pkg["binary"] = Value::Object(new_binary.clone());

    // Safely get package name and version
    let name = pkg
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("unknown");
    let version = pkg
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    log_info(&format!(
        "{name}@{version} download from binary mirror: {new_binary:?}"
    ));
}

async fn handle_node_pre_gyp_versioning(dir: &Path) -> Result<()> {
    let versioning_file = dir.join("node_modules/node-pre-gyp/lib/util/versioning.js");
    if versioning_file.exists() {
        let content = fs::read_to_string(&versioning_file)
            .await
            .context("Failed to read versioning.js")?;

        let new_content = content.replace(
            "if (protocol === 'http:') {",
            "if (false && protocol === 'http:') { // hack by npminstall",
        );

        fs::write(&versioning_file, new_content)
            .await
            .context("Failed to write versioning.js")?;
    }
    Ok(())
}

fn should_handle_replace_host(binary_mirror: &Map<String, Value>) -> bool {
    (binary_mirror.contains_key("replaceHost") && binary_mirror.contains_key("host"))
        || binary_mirror.contains_key("replaceHostMap")
        || binary_mirror.contains_key("replaceHostRegExpMap")
}

fn get_replace_host_files(binary_mirror: &Map<String, Value>) -> Vec<&str> {
    binary_mirror
        .get("replaceHostFiles")
        .and_then(|f| f.as_array())
        .map(|f| f.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["lib/index.js", "lib/install.js"])
}

fn replace_with_regex(content: &str, replace_map: &Value) -> Result<String> {
    let mut result = content.to_string();
    for (pattern, replacement) in replace_map.as_object().unwrap() {
        let re = Regex::new(pattern).with_context(|| format!("Invalid regex pattern {pattern}"))?;
        result = re
            .replace_all(&result, replacement.as_str().unwrap())
            .to_string();
    }
    Ok(result)
}

fn replace_with_map(content: &str, binary_mirror: &Map<String, Value>) -> Result<String> {
    let replace_map = if let Some(map) = binary_mirror.get("replaceHostMap") {
        map.as_object().unwrap().clone()
    } else {
        let mut map = Map::new();
        let hosts = binary_mirror
            .get("replaceHost")
            .and_then(|h| h.as_array())
            .map(|h| h.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_else(|| vec![binary_mirror["host"].as_str().unwrap()]);

        for host in hosts {
            map.insert(
                host.to_string(),
                Value::String(binary_mirror["host"].as_str().unwrap().to_string()),
            );
        }
        map
    };

    let mut result = content.to_string();
    for (from, to) in replace_map {
        result = result.replace(&from, to.as_str().unwrap());
    }
    Ok(result)
}

async fn handle_replace_host(dir: &Path, binary_mirror: &Map<String, Value>) -> Result<()> {
    if !should_handle_replace_host(binary_mirror) {
        return Ok(());
    }

    let replace_host_files = get_replace_host_files(binary_mirror);
    for file in replace_host_files {
        let file_path = dir.join(file);
        if file_path.exists() {
            let content = fs::read_to_string(&file_path)
                .await
                .context("Failed to read file")?;

            let new_content = if let Some(replace_map) = binary_mirror.get("replaceHostRegExpMap") {
                replace_with_regex(&content, replace_map)?
            } else {
                replace_with_map(&content, binary_mirror)?
            };

            fs::write(&file_path, new_content)
                .await
                .context("Failed to write file")?;
        }
    }
    Ok(())
}

async fn handle_cypress(
    dir: &Path,
    pkg: &Value,
    binary_mirror: &Map<String, Value>,
    target_os: Option<&str>,
) -> Result<()> {
    if pkg["name"].as_str().unwrap() != "cypress" {
        return Ok(());
    }

    let default_platforms = serde_json::json!({
        "darwin": "osx64",
        "linux": "linux64",
        "win32": "win64"
    });

    let platforms = if let Some(new_platforms) = binary_mirror.get("newPlatforms") {
        if matches(">=3.3.0", pkg["version"].as_str().unwrap()) {
            new_platforms
        } else {
            &default_platforms
        }
    } else {
        &default_platforms
    };

    let os = target_os.unwrap_or(std::env::consts::OS);
    if let Some(target_platform) = platforms[os].as_str() {
        let download_file = dir.join("lib/tasks/download.js");
        if download_file.exists() {
            let content = fs::read_to_string(&download_file)
                .await
                .context("Failed to read download.js")?;

            let new_content = content
                .replace(
                    "return version ? prepend(`desktop/${version}`) : prepend('desktop')",
                    &format!(
                        "return \"{}\" + version + \"/{}/cypress.zip\"; // hack by npminstall",
                        binary_mirror["host"].as_str().unwrap(),
                        target_platform
                    ),
                )
                .replace(
                    "return version ? prepend('desktop/' + version) : prepend('desktop');",
                    &format!(
                        "return \"{}\" + version + \"/{}/cypress.zip\"; // hack by npminstall",
                        binary_mirror["host"].as_str().unwrap(),
                        target_platform
                    ),
                );

            fs::write(&download_file, new_content)
                .await
                .context("Failed to write download.js")?;
        }
    }

    Ok(())
}

pub async fn update_package_binary(dir: &Path, name: &str) -> Result<()> {
    let config = load_config().await?;

    let mirrors = config["mirrors"]["china"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid binary mirror config format"))?;

    if let Some(binary_mirror) = mirrors.get(name) {
        let binary_mirror = binary_mirror
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Invalid binary mirror format"))?;

        // Read package.json
        let pkg_path = dir.join("package.json");
        let mut pkg = load_package_json_from_path(dir)?;

        // has install script and not replaceHostFiles
        let should_update_binary = if let Some(scripts) = pkg["scripts"].as_object() {
            scripts.contains_key("install") && !binary_mirror.contains_key("replaceHostFiles")
        } else {
            false
        };

        // detect node-pre-gyp
        let should_handle_node_pre_gyp = if let Some(scripts) = pkg["scripts"].as_object() {
            scripts
                .get("install")
                .and_then(|s| s.as_str())
                .map(|s| s.contains("node-pre-gyp install"))
                .unwrap_or(false)
        } else {
            false
        };

        // update binary config
        if should_update_binary {
            update_binary_config(&mut pkg, binary_mirror);
        }

        // process node-pre-gyp
        if should_handle_node_pre_gyp {
            handle_node_pre_gyp_versioning(dir).await?;
        }

        handle_replace_host(dir, binary_mirror).await?;
        handle_cypress(dir, &pkg, binary_mirror, None).await?;

        // Write updated package.json
        fs::write(pkg_path, serde_json::to_string_pretty(&pkg).unwrap())
            .await
            .context("Failed to write package.json")?;
    }

    Ok(())
}

pub async fn get_envs() -> Option<&'static Map<String, Value>> {
    match load_config().await {
        Ok(_) => CONFIG
            .get()
            .and_then(|config| config["mirrors"]["china"]["ENVS"].as_object()),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_update_binary_config() {
        let mut pkg = json!({
            "name": "test-package",
            "version": "1.0.0",
            "binary": {
                "existing": "value"
            }
        });

        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "host": "https://example.com",
            "replaceHostFiles": ["test.js"],
            "newKey": "newValue"
        }))
        .unwrap();

        update_binary_config(&mut pkg, &binary_mirror);

        assert_eq!(pkg["binary"]["existing"].as_str(), Some("value"));
        assert_eq!(pkg["binary"]["host"].as_str(), Some("https://example.com"));
        assert_eq!(pkg["binary"]["newKey"].as_str(), Some("newValue"));
        assert!(
            !pkg["binary"]
                .as_object()
                .unwrap()
                .contains_key("replaceHostFiles")
        );
    }

    #[tokio::test]
    async fn test_should_handle_replace_host() {
        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "replaceHost": ["old.com"],
            "host": "new.com"
        }))
        .unwrap();
        assert!(should_handle_replace_host(&binary_mirror));

        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "replaceHostMap": {
                "old.com": "new.com"
            }
        }))
        .unwrap();
        assert!(should_handle_replace_host(&binary_mirror));

        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "replaceHostRegExpMap": {
                "old\\.com": "new.com"
            }
        }))
        .unwrap();
        assert!(should_handle_replace_host(&binary_mirror));

        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "host": "new.com"
        }))
        .unwrap();
        assert!(!should_handle_replace_host(&binary_mirror));
    }

    #[tokio::test]
    async fn test_get_replace_host_files() {
        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "replaceHostFiles": ["custom.js"]
        }))
        .unwrap();
        assert_eq!(get_replace_host_files(&binary_mirror), vec!["custom.js"]);

        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({})).unwrap();
        assert_eq!(
            get_replace_host_files(&binary_mirror),
            vec!["lib/index.js", "lib/install.js"]
        );
    }

    #[tokio::test]
    async fn test_replace_with_regex() {
        let content = "Visit old.com and old.com";
        let replace_map = json!({
            "old\\.com": "new.com"
        });

        let result = replace_with_regex(content, &replace_map).unwrap();
        assert_eq!(result, "Visit new.com and new.com");
    }

    #[tokio::test]
    async fn test_replace_with_map() {
        let content = "Visit old.com and old.com";
        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "replaceHostMap": {
                "old.com": "new.com"
            }
        }))
        .unwrap();

        let result = replace_with_map(content, &binary_mirror).unwrap();
        assert_eq!(result, "Visit new.com and new.com");
    }

    #[tokio::test]
    async fn test_handle_cypress() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();
        println!("Test directory: {:?}", dir);

        // Create necessary directory structure
        let lib_tasks_dir = dir.join("lib/tasks");
        std::fs::create_dir_all(&lib_tasks_dir).unwrap();
        println!("Created directory: {:?}", lib_tasks_dir);

        // Create test download.js file
        let download_file = lib_tasks_dir.join("download.js");
        let original_content = r#"
            return version ? prepend(`desktop/${version}`) : prepend('desktop');
            return version ? prepend('desktop/' + version) : prepend('desktop');
        "#;
        std::fs::write(&download_file, original_content).unwrap();
        println!("Created file: {:?}", download_file);

        let pkg = json!({
            "name": "cypress",
            "version": "3.3.0"
        });

        let binary_mirror = serde_json::from_value::<Map<String, Value>>(json!({
            "host": "https://example.com",
            "newPlatforms": {
                "darwin": "osx64",
                "linux": "linux64",
                "win32": "win64"
            }
        }))
        .unwrap();

        handle_cypress(dir, &pkg, &binary_mirror, Some("darwin"))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&download_file).unwrap();
        println!("File content after modification:\n{}", content);

        assert!(
            content.contains("https://example.com"),
            "Content should contain host URL"
        );
        assert!(content.contains("osx64"), "Content should contain platform");
        assert!(
            !content.contains("prepend"),
            "Content should not contain original prepend calls"
        );
    }

    #[tokio::test]
    async fn test_update_package_binary_fsevents() {
        // Create a temporary directory for testing
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create package.json
        let pkg_json = json!({
            "name": "fsevents",
            "version": "2.3.3",
            "scripts": {
                "install": "node-gyp rebuild"
            }
        });

        let pkg_path = dir.join("package.json");
        std::fs::write(&pkg_path, pkg_json.to_string()).unwrap();

        // Call the function
        update_package_binary(dir, "fsevents").await.unwrap();

        // Read the updated package.json
        let updated_pkg: Value =
            serde_json::from_str(&std::fs::read_to_string(pkg_path).unwrap()).unwrap();

        // Should not change version
        assert_eq!(updated_pkg["name"], "fsevents");
        assert_eq!(updated_pkg["version"], "2.3.3");
    }
}
