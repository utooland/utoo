use crate::fs;
use crate::helper::ruborist_context::Context as RuboristContext;
use crate::util::json::read_json_file;
use crate::util::user_config::get_registry;
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
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

/// Per-package binary mirror settings.
#[derive(Debug, Default, Clone, Deserialize, serde::Serialize)]
struct BinaryMirror {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    /// Hosts to rewrite to `host` (used when `replaceHostMap` is absent).
    #[serde(
        rename = "replaceHost",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    replace_host: Option<Vec<String>>,
    /// Files to rewrite hosts in (defaults to lib/index.js + lib/install.js).
    /// Excluded from the merge into the package's `binary` config.
    #[serde(
        rename = "replaceHostFiles",
        default,
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

static CONFIG: OnceCell<BinaryMirrorConfig> = OnceCell::const_new();
/// Cached result of whether we should skip binary mirror envs
static SKIP_BINARY_MIRROR: OnceLock<bool> = OnceLock::new();

/// On-disk cache TTL for `binary-mirror-config`. The config changes rarely;
/// without this cache every warm install on a non-npmjs registry pays a
/// network round trip (TLS + manifest fetch) before the first mirror-matched
/// package can finish cloning. 6h keeps worst-case staleness of a new mirror
/// entry within the same workday.
const DISK_CACHE_TTL_SECS: u64 = 21600; // 6 hours

fn disk_cache_path() -> std::path::PathBuf {
    crate::util::cache::get_cache_dir().join("_binary-mirror-config.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Return the cached config if still fresh. `BinaryMirrorConfig` is
/// deserialize-only, so the cache stores the raw `binary-mirror-config`
/// manifest and we re-parse it here — the same path `load_config` takes for a
/// freshly fetched manifest.
fn read_disk_cache() -> Option<BinaryMirrorConfig> {
    let raw = std::fs::read(disk_cache_path()).ok()?;
    let cached: Value = serde_json::from_slice(&raw).ok()?;
    let fetched_at = cached.get("fetched_at")?.as_u64()?;
    if now_secs().saturating_sub(fetched_at) > DISK_CACHE_TTL_SECS {
        return None;
    }
    serde_json::from_value(cached.get("manifest")?.clone()).ok()
}

/// Persist the raw manifest (not the typed config) with a fetch timestamp, so
/// a later run rebuilds the config without touching the network.
fn write_disk_cache(bytes: &[u8]) {
    if let Ok(manifest) = serde_json::from_slice::<Value>(bytes) {
        let entry = serde_json::json!({ "fetched_at": now_secs(), "manifest": manifest });
        let path = disk_cache_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = crate::util::json::write_compact_sync(&path, &entry) {
            tracing::debug!("Failed to write binary mirror config cache: {e}");
        }
    }
}

/// Kick off the config fetch in the background so it overlaps the install
/// pipeline instead of stalling the first mirror-matched package (the
/// `OnceCell` dedupes against the eventual in-line caller).
pub fn warm_binary_mirror_config() {
    if should_skip_binary_mirror() {
        return;
    }
    tokio::spawn(async {
        if let Err(e) = load_config().await {
            tracing::debug!("Binary mirror config warmup failed: {e}");
        }
    });
}

async fn load_config() -> Result<&'static BinaryMirrorConfig> {
    CONFIG
        .get_or_try_init(|| async {
            // Serve from the on-disk cache while fresh — a fully warm install
            // must not touch the network for a config that changes rarely.
            if let Some(config) = tokio::task::spawn_blocking(read_disk_cache)
                .await
                .ok()
                .flatten()
            {
                return Ok(config);
            }
            // Go through the registry client so URL construction and private-
            // registry auth are handled in one place rather than hand-rolled.
            // `binary-mirror-config@latest` is a normal version manifest whose
            // `mirrors` field carries the config.
            let bytes = RuboristContext::registry()
                .await
                .fetch_version_manifest_bytes("binary-mirror-config", "latest")
                .await
                .context("Failed to fetch binary mirror config")?;
            let config: BinaryMirrorConfig =
                serde_json::from_slice(&bytes).context("Failed to parse binary mirror config")?;
            tokio::task::spawn_blocking(move || write_disk_cache(&bytes));
            Ok(config)
        })
        .await
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
