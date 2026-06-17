use crate::fs;
use crate::helper::ruborist_context::Context as RuboristContext;
use crate::util::json::read_json_file;
use crate::util::user_config::get_registry;
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::result::Result as StdResult;
use std::sync::OnceLock;
use tokio::sync::OnceCell;
use utoo_ruborist::registry::is_npm_registry;
use utoo_ruborist::semver::matches;

/// The `binary-mirror-config` package (cnpm), parsed from its version
/// manifest's `mirrors` field. We own this schema, so it is modeled
/// explicitly rather than poked at through `serde_json::Value`. Every field
/// is optional/defaulted so a config that grows new sections never fails the
/// whole parse (a parse failure disables mirroring for the whole install).
#[derive(Debug, Default, Deserialize)]
struct BinaryMirrorConfig {
    #[serde(default)]
    mirrors: Mirrors,
}

#[derive(Debug, Default, Deserialize)]
struct Mirrors {
    #[serde(default)]
    china: ChinaMirror,
}

/// The `mirrors.china` section: shared env overrides plus one entry per
/// package whose prebuilt binaries we redirect to the China CDN.
#[derive(Debug, Default, Deserialize)]
struct ChinaMirror {
    /// Environment variables exported into every install/build script.
    #[serde(rename = "ENVS", default)]
    envs: BTreeMap<String, String>,
    /// Per-package mirror settings, keyed by package name (every key under
    /// `china` other than `ENVS`).
    #[serde(flatten)]
    packages: BTreeMap<String, BinaryMirror>,
}

/// npm's `binary-mirror-config` lets `replaceHost`/`replaceHostFiles` be either
/// a single string or an array of strings (e.g. `flow-bin.replaceHost` is a
/// bare string). Normalize both shapes to a `Vec<String>`; a strict
/// `Option<Vec<String>>` would reject the string form and fail the whole parse.
fn de_string_or_seq<'de, D>(deserializer: D) -> StdResult<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrSeq {
        String(String),
        Seq(Vec<String>),
    }
    Ok(match Option::<StringOrSeq>::deserialize(deserializer)? {
        None => None,
        Some(StringOrSeq::String(s)) => Some(vec![s]),
        Some(StringOrSeq::Seq(v)) => Some(v),
    })
}

/// Per-package binary mirror settings.
#[derive(Debug, Default, Clone, Deserialize, serde::Serialize)]
struct BinaryMirror {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    /// Hosts to rewrite to `host` (used when `replaceHostMap` is absent).
    #[serde(
        rename = "replaceHost",
        default,
        deserialize_with = "de_string_or_seq",
        skip_serializing_if = "Option::is_none"
    )]
    replace_host: Option<Vec<String>>,
    /// Files to rewrite hosts in (defaults to lib/index.js + lib/install.js).
    /// Excluded from the merge into the package's `binary` config.
    #[serde(
        rename = "replaceHostFiles",
        default,
        deserialize_with = "de_string_or_seq",
        skip_serializing_if = "Option::is_none"
    )]
    replace_host_files: Option<Vec<String>>,
    /// Explicit from→to host rewrite map.
    #[serde(
        rename = "replaceHostMap",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    replace_host_map: Option<BTreeMap<String, String>>,
    /// Regex→replacement host rewrite map.
    #[serde(
        rename = "replaceHostRegExpMap",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    replace_host_regexp_map: Option<BTreeMap<String, String>>,
    /// Cypress platform map (`os` → platform slug) for versions >= 3.3.0.
    #[serde(
        rename = "newPlatforms",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    new_platforms: Option<BTreeMap<String, String>>,
    /// Open-ended node-pre-gyp / node-gyp knobs (module_name, remote_path, …)
    /// passed through verbatim into the package's `binary` config.
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Cached config-load outcome. Stores `Result` rather than the bare config so
/// a fetch/parse *failure* is memoized too: `get_or_try_init` would discard an
/// `Err` and re-run the closure on the next call, and `update_package_binary`
/// is called once per package — a single bad config would otherwise trigger a
/// `binary-mirror-config` network fetch for every package in the tree.
static CONFIG: OnceCell<Result<BinaryMirrorConfig, String>> = OnceCell::const_new();
/// Cached result of whether we should skip binary mirror envs
static SKIP_BINARY_MIRROR: OnceLock<bool> = OnceLock::new();

async fn load_config() -> Result<&'static BinaryMirrorConfig> {
    CONFIG
        .get_or_init(|| async {
            // Go through the registry client so URL construction and private-
            // registry auth are handled in one place rather than hand-rolled.
            // `binary-mirror-config@latest` is a normal version manifest whose
            // `mirrors` field carries the config.
            let bytes = match RuboristContext::registry()
                .await
                .fetch_version_manifest_bytes("binary-mirror-config", "latest")
                .await
            {
                Ok(bytes) => bytes,
                Err(e) => return Err(format!("Failed to fetch binary mirror config: {e:#}")),
            };
            serde_json::from_slice(&bytes)
                .map_err(|e| format!("Failed to parse binary mirror config: {e}"))
        })
        .await
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn update_binary_config(pkg: &mut Value, binary_mirror: &BinaryMirror) {
    // Get existing binary configuration
    let mut new_binary = match pkg.get("binary").and_then(Value::as_object) {
        Some(obj) => obj.clone(),
        None => Map::new(),
    };

    // Merge the mirror entry's keys on top, except `replaceHostFiles` (which
    // selects rewrite targets and is not a binary-download knob). Serializing
    // the typed entry reproduces its exact wire keys (structural fields +
    // passed-through `extra`).
    if let Ok(Value::Object(entry)) = serde_json::to_value(binary_mirror) {
        for (key, value) in entry {
            if key != "replaceHostFiles" {
                new_binary.insert(key, value);
            }
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

fn should_handle_replace_host(binary_mirror: &BinaryMirror) -> bool {
    (binary_mirror.replace_host.is_some() && binary_mirror.host.is_some())
        || binary_mirror.replace_host_map.is_some()
        || binary_mirror.replace_host_regexp_map.is_some()
}

fn get_replace_host_files(binary_mirror: &BinaryMirror) -> Vec<&str> {
    match &binary_mirror.replace_host_files {
        Some(files) => files.iter().map(String::as_str).collect(),
        None => vec!["lib/index.js", "lib/install.js"],
    }
}

/// The mirror host. A `replaceHost`/cypress rewrite needs a target host, so a
/// missing one is a config error, not a panic.
fn mirror_host(binary_mirror: &BinaryMirror) -> Result<&str> {
    binary_mirror
        .host
        .as_deref()
        .context("binary-mirror config missing string `host`")
}

fn replace_with_regex(content: &str, replace_map: &BTreeMap<String, String>) -> Result<String> {
    let mut result = content.to_string();
    for (pattern, replacement) in replace_map {
        let re = Regex::new(pattern).with_context(|| format!("Invalid regex pattern {pattern}"))?;
        result = re.replace_all(&result, replacement.as_str()).to_string();
    }
    Ok(result)
}

fn replace_with_map(content: &str, binary_mirror: &BinaryMirror) -> Result<String> {
    let replace_map = if let Some(map) = &binary_mirror.replace_host_map {
        map.clone()
    } else {
        let host = mirror_host(binary_mirror)?;
        let hosts = match &binary_mirror.replace_host {
            Some(hosts) => hosts.iter().map(String::as_str).collect(),
            None => vec![host],
        };
        hosts
            .into_iter()
            .map(|from| (from.to_string(), host.to_string()))
            .collect()
    };

    let mut result = content.to_string();
    for (from, to) in &replace_map {
        result = result.replace(from, to);
    }
    Ok(result)
}

async fn handle_replace_host(dir: &Path, binary_mirror: &BinaryMirror) -> Result<()> {
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

            let new_content = match &binary_mirror.replace_host_regexp_map {
                Some(regexp_map) => replace_with_regex(&content, regexp_map)?,
                None => replace_with_map(&content, binary_mirror)?,
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
    binary_mirror: &BinaryMirror,
    target_os: Option<&str>,
) -> Result<()> {
    if pkg.get("name").and_then(Value::as_str) != Some("cypress") {
        return Ok(());
    }

    let default_platforms: BTreeMap<&str, &str> = [
        ("darwin", "osx64"),
        ("linux", "linux64"),
        ("win32", "win64"),
    ]
    .into();

    // Cypress >= 3.3.0 uses the config's `newPlatforms` slugs; older versions
    // keep the legacy defaults.
    let use_new_platforms = match (
        &binary_mirror.new_platforms,
        pkg.get("version").and_then(Value::as_str),
    ) {
        (Some(new_platforms), Some(version)) if matches(">=3.3.0", version) => Some(new_platforms),
        _ => None,
    };

    let os = target_os.unwrap_or(std::env::consts::OS);
    let target_platform = match use_new_platforms {
        Some(new_platforms) => new_platforms.get(os).map(String::as_str),
        None => default_platforms.get(os).copied(),
    };

    if let Some(target_platform) = target_platform {
        let download_file = dir.join("lib/tasks/download.js");
        if fs::try_exists(&download_file).await? {
            let content = fs::read_to_string(&download_file)
                .await
                .context("Failed to read download.js")?;

            let host = mirror_host(binary_mirror)?;
            let mirror_return = format!(
                "return \"{host}\" + version + \"/{target_platform}/cypress.zip\"; // hack by npminstall"
            );
            let new_content = content
                .replace(
                    "return version ? prepend(`desktop/${version}`) : prepend('desktop')",
                    &mirror_return,
                )
                .replace(
                    "return version ? prepend('desktop/' + version) : prepend('desktop');",
                    &mirror_return,
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

    let Some(binary_mirror) = config.mirrors.china.packages.get(name) else {
        return Ok(());
    };

    // Read package.json as raw Value for in-place mutation — it is an
    // arbitrary third-party manifest we patch and write back, so it stays
    // untyped (typing would risk dropping fields we don't model).
    let pkg_path = dir.join("package.json");
    let mut pkg: Value = read_json_file(&pkg_path).await?;

    // has install script and not replaceHostFiles
    let should_update_binary = pkg["scripts"].as_object().is_some_and(|scripts| {
        scripts.contains_key("install") && binary_mirror.replace_host_files.is_none()
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
    fs::write(pkg_path, serde_json::to_string_pretty(&pkg)?)
        .await
        .context("Failed to write package.json")?;

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

pub async fn get_envs() -> Option<&'static BTreeMap<String, String>> {
    // Skip binary mirror envs when using official npm registry
    if should_skip_binary_mirror() {
        return None;
    }

    match load_config().await {
        Ok(config) => {
            let envs = &config.mirrors.china.envs;
            (!envs.is_empty()).then_some(envs)
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

        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
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
        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
            "replaceHost": ["old.com"],
            "host": "new.com"
        }))
        .unwrap();
        assert!(should_handle_replace_host(&binary_mirror));

        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
            "replaceHostMap": {
                "old.com": "new.com"
            }
        }))
        .unwrap();
        assert!(should_handle_replace_host(&binary_mirror));

        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
            "replaceHostRegExpMap": {
                "old\\.com": "new.com"
            }
        }))
        .unwrap();
        assert!(should_handle_replace_host(&binary_mirror));

        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
            "host": "new.com"
        }))
        .unwrap();
        assert!(!should_handle_replace_host(&binary_mirror));
    }

    #[tokio::test]
    async fn test_get_replace_host_files() {
        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
            "replaceHostFiles": ["custom.js"]
        }))
        .unwrap();
        assert_eq!(get_replace_host_files(&binary_mirror), vec!["custom.js"]);

        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({})).unwrap();
        assert_eq!(
            get_replace_host_files(&binary_mirror),
            vec!["lib/index.js", "lib/install.js"]
        );
    }

    #[test]
    fn test_replace_host_accepts_string_or_array() {
        // npm's config has `flow-bin.replaceHost` as a bare string; the strict
        // `Vec<String>` form used to fail the whole config parse here.
        let from_string = serde_json::from_value::<BinaryMirror>(json!({
            "replaceHost": "https://github.com/facebook/flow/releases/download/v",
            "replaceHostFiles": "lib/install.js"
        }))
        .unwrap();
        assert_eq!(
            from_string.replace_host.as_deref(),
            Some(["https://github.com/facebook/flow/releases/download/v".to_string()].as_slice())
        );
        assert_eq!(
            from_string.replace_host_files.as_deref(),
            Some(["lib/install.js".to_string()].as_slice())
        );

        let from_array = serde_json::from_value::<BinaryMirror>(json!({
            "replaceHost": ["a.com", "b.com"]
        }))
        .unwrap();
        assert_eq!(
            from_array.replace_host.as_deref(),
            Some(["a.com".to_string(), "b.com".to_string()].as_slice())
        );
    }

    #[test]
    fn test_full_config_with_string_replace_host_parses() {
        // A whole-config parse must not fail just because one entry uses the
        // string form of `replaceHost` (regression: it disabled mirroring for
        // every package and re-fetched the config per package).
        let config: BinaryMirrorConfig = serde_json::from_value(json!({
            "mirrors": {
                "china": {
                    "ENVS": { "SASS_BINARY_SITE": "https://npmmirror.com/mirrors/node-sass" },
                    "flow-bin": {
                        "replaceHost": "https://github.com/facebook/flow/releases/download/v",
                        "host": "https://npmmirror.com/mirrors/flow"
                    },
                    "sharp": { "replaceHostFiles": ["lib/libvips.js"] }
                }
            }
        }))
        .unwrap();
        assert!(config.mirrors.china.packages.contains_key("flow-bin"));
        assert!(config.mirrors.china.packages.contains_key("sharp"));
        assert!(config.mirrors.china.envs.contains_key("SASS_BINARY_SITE"));
    }

    #[tokio::test]
    async fn test_replace_with_regex() {
        let content = "Visit old.com and old.com";
        let replace_map = BTreeMap::from([("old\\.com".to_string(), "new.com".to_string())]);

        let result = replace_with_regex(content, &replace_map).unwrap();
        assert_eq!(result, "Visit new.com and new.com");
    }

    #[tokio::test]
    async fn test_replace_with_map() {
        let content = "Visit old.com and old.com";
        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
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

        let binary_mirror = serde_json::from_value::<BinaryMirror>(json!({
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
