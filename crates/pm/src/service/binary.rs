use crate::fs;
use crate::service::auth;
use crate::util::http::client;
use crate::util::json::read_json_file;
use crate::util::user_config::get_registry;
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::OnceLock;
use tokio::sync::OnceCell;
use utoo_ruborist::registry::is_npm_registry;
use utoo_ruborist::semver::matches;

static CONFIG: OnceCell<Value> = OnceCell::const_new();
/// Cached result of whether we should skip binary mirror envs
static SKIP_BINARY_MIRROR: OnceLock<bool> = OnceLock::new();

async fn load_config() -> Result<&'static Value> {
    CONFIG
        .get_or_try_init(|| async {
            let registry = get_registry();
            let url = format!("{registry}/binary-mirror-config/latest");
            let mut request = client().get(&url);
            if let Some(token) = auth::token_for_url(&url).await {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .context("Failed to fetch binary mirror config")?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!("HTTP status: {}", response.status()));
            }

            response
                .json()
                .await
                .context("Failed to parse binary mirror config")
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

    tracing::debug!("{name}@{version} download from binary mirror: {new_binary:?}");
}

async fn handle_node_pre_gyp_versioning(dir: &Path) -> Result<()> {
    let versioning_file = dir.join("node_modules/node-pre-gyp/lib/util/versioning.js");
    if fs::try_exists(&versioning_file).await? {
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
        if fs::try_exists(&file_path).await? {
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
        if fs::try_exists(&download_file).await? {
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
    // npm.org has no China mirror layer — skip alongside `get_envs`.
    if should_skip_binary_mirror() {
        return Ok(());
    }

    // A missing/unreachable binary-mirror-config (e.g. a private registry that
    // doesn't host it) must not fail the install — the china-mirror rewrite is
    // an optimization. Skip gracefully, matching `get_envs`.
    let config = match load_config().await {
        Ok(config) => config,
        Err(e) => {
            tracing::debug!("Binary mirror config unavailable, skipping: {e}");
            return Ok(());
        }
    };

    let mirrors = config["mirrors"]["china"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid binary mirror config format"))?;

    if let Some(binary_mirror) = mirrors.get(name) {
        let binary_mirror = binary_mirror
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Invalid binary mirror format"))?;

        // Read package.json as raw Value for in-place mutation
        let pkg_path = dir.join("package.json");
        let mut pkg: Value = read_json_file(&pkg_path).await?;

        // has install script and not replaceHostFiles
        let should_update_binary = pkg["scripts"].as_object().is_some_and(|scripts| {
            scripts.contains_key("install") && !binary_mirror.contains_key("replaceHostFiles")
        });

        // detect node-pre-gyp
        let should_handle_node_pre_gyp = pkg["scripts"]
            .as_object()
            .and_then(|scripts| scripts.get("install"))
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.contains("node-pre-gyp install"));

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

fn should_skip_binary_mirror() -> bool {
    *SKIP_BINARY_MIRROR.get_or_init(|| {
        let registry = get_registry();
        let skip = is_npm_registry(&registry);
        if skip {
            tracing::debug!("Skipping binary mirror envs for npm registry: {}", registry);
        }
        skip
    })
}

pub async fn get_envs() -> Option<&'static Map<String, Value>> {
    // Skip binary mirror envs when using official npm registry
    if should_skip_binary_mirror() {
        return None;
    }

    match load_config().await {
        Ok(_) => {
            let config = CONFIG.get();
            config.and_then(|config| config["mirrors"]["china"]["ENVS"].as_object())
        }
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
        println!("Test directory: {dir:?}");

        // Create necessary directory structure
        let lib_tasks_dir = dir.join("lib/tasks");
        fs::create_dir_all(&lib_tasks_dir).await.unwrap();
        println!("Created directory: {lib_tasks_dir:?}");

        // Create test download.js file
        let download_file = lib_tasks_dir.join("download.js");
        let original_content = r#"
            return version ? prepend(`desktop/${version}`) : prepend('desktop');
            return version ? prepend('desktop/' + version) : prepend('desktop');
        "#;
        fs::write(&download_file, original_content).await.unwrap();
        println!("Created file: {download_file:?}");

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

        let content = fs::read_to_string(&download_file).await.unwrap();
        println!("File content after modification:\n{content}");

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
        fs::write(&pkg_path, pkg_json.to_string()).await.unwrap();

        // Call the function
        update_package_binary(dir, "fsevents").await.unwrap();

        // Read the updated package.json
        let updated_pkg: Value =
            serde_json::from_str(&fs::read_to_string(pkg_path).await.unwrap()).unwrap();

        // Should not change version
        assert_eq!(updated_pkg["name"], "fsevents");
        assert_eq!(updated_pkg["version"], "2.3.3");
    }
}
