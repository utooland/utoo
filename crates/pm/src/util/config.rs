use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use super::registry::{REGISTRY_NPMMIRROR, select_fastest_registry};
use super::save_type::OmitType;

pub type ConfigResult<T> = Result<T>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    values: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    arrays: HashMap<String, Vec<String>>,
}

// global config path is ~/.utoo/config.toml
// local config path is .utoo/config.toml
impl Config {
    pub async fn load(global: bool) -> ConfigResult<Self> {
        if global {
            return Self::load_from_path(&Self::global_config_path()?).await;
        }

        let mut config = Self::load_from_path(&Self::global_config_path()?).await?;
        let local_path = Self::local_config_path()?;
        if crate::fs::try_exists(&local_path).await? {
            let local_config = Self::load_from_path(&local_path).await?;
            config.values.extend(local_config.values);
            config.arrays.extend(local_config.arrays);
        }
        Ok(config)
    }

    pub fn set(&mut self, key: &str, value: String, global: bool) -> ConfigResult<()> {
        self.values.insert(key.to_string(), value);
        self.save(global)
    }

    pub fn get(&self, key: &str) -> ConfigResult<Option<String>> {
        Ok(self.values.get(key).cloned())
    }

    pub fn get_array(&self, key: &str) -> Option<&Vec<String>> {
        self.arrays.get(key)
    }

    pub fn set_array(&mut self, key: &str, value: Vec<String>, global: bool) -> ConfigResult<()> {
        self.arrays.insert(key.to_string(), value);
        self.save(global)
    }

    async fn load_from_path(path: &Path) -> ConfigResult<Self> {
        if !crate::fs::try_exists(path).await? {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, global: bool) -> ConfigResult<()> {
        let path = if global {
            Self::global_config_path()?
        } else {
            Self::local_config_path()?
        };

        // ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    fn global_config_path() -> ConfigResult<PathBuf> {
        Ok(dirs::home_dir()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")
            })?
            .join(".utoo/config.toml"))
    }

    fn local_config_path() -> ConfigResult<PathBuf> {
        Ok(std::env::current_dir()?.join(".utoo.toml"))
    }

    pub fn get_global_config_path(&self) -> ConfigResult<PathBuf> {
        Self::global_config_path()
    }

    pub fn get_local_config_path(&self) -> ConfigResult<PathBuf> {
        Self::local_config_path()
    }

    pub fn list(&self) -> ConfigResult<impl Iterator<Item = (&String, &String)>> {
        Ok(self.values.iter())
    }

    pub fn list_arrays(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.arrays.iter()
    }
}

// Configuration value parser for different types
trait ConfigValueParser<T> {
    fn parse_config_value(&self, value: &str) -> T;
}

struct ConfigValue<T> {
    value: OnceLock<T>,
    key: &'static str,
    default: T,
}

impl<T: Clone + Debug + 'static> ConfigValue<T> {
    const fn new(key: &'static str, default: T) -> Self {
        Self {
            value: OnceLock::new(),
            key,
            default,
        }
    }

    fn set(&self, new_value: Option<T>) {
        if let Some(value) = new_value {
            tracing::debug!("set {}: {:?}", self.key, value);
            let _ = self.value.set(value);
        }
    }

    async fn get(&self) -> T
    where
        Self: ConfigValueParser<T>,
    {
        if let Some(value) = self.value.get() {
            return value.clone();
        }

        // load from config - refactored for better readability
        let config_result = Config::load(false).await;
        if let Ok(config) = config_result {
            let value_result = config.get(self.key);
            if let Ok(Some(value)) = value_result {
                let parsed_value = self.parse_config_value(&value);
                let _ = self.value.set(parsed_value.clone());
                return parsed_value;
            }
        }

        self.default.clone()
    }

    fn get_sync(&self) -> T
    where
        Self: ConfigValueParser<T>,
    {
        if let Some(value) = self.value.get() {
            return value.clone();
        }

        // If not set, return the default value
        self.default.clone()
    }
}

impl ConfigValueParser<String> for ConfigValue<String> {
    fn parse_config_value(&self, value: &str) -> String {
        value.to_string()
    }
}

impl ConfigValueParser<bool> for ConfigValue<bool> {
    fn parse_config_value(&self, value: &str) -> bool {
        value.to_lowercase() == "true"
    }
}

impl ConfigValueParser<usize> for ConfigValue<usize> {
    fn parse_config_value(&self, value: &str) -> usize {
        value.parse().unwrap_or(self.default)
    }
}

static REGISTRY: LazyLock<ConfigValue<String>> =
    LazyLock::new(|| ConfigValue::new("registry", REGISTRY_NPMMIRROR.to_string()));

static LEGACY_PEER_DEPS: LazyLock<ConfigValue<bool>> =
    LazyLock::new(|| ConfigValue::new("legacy-peer-deps", true));

static CACHE_DIR: LazyLock<ConfigValue<String>> = LazyLock::new(|| {
    let default_cache = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache/nm")
        .to_string_lossy()
        .to_string();
    ConfigValue::new("cache-dir", default_cache)
});

pub async fn set_registry(registry: Option<String>) {
    // Priority: CLI argument > UTOO_REGISTRY env > config > auto-select
    let final_registry = if let Some(reg) = registry {
        tracing::debug!("Using registry from CLI: {}", reg);
        Some(reg)
    } else if let Ok(env_reg) = std::env::var("UTOO_REGISTRY")
        && !env_reg.is_empty()
    {
        tracing::debug!("Using registry from env: {}", env_reg);
        Some(env_reg)
    } else {
        let config_registry = Config::load(false)
            .await
            .ok()
            .and_then(|c| c.get("registry").ok().flatten());

        if let Some(ref reg) = config_registry {
            tracing::debug!("Using registry from config: {}", reg);
            config_registry
        } else {
            // No config, auto-select fastest registry
            Some(select_fastest_registry().await)
        }
    };

    REGISTRY.set(final_registry);

    // Auto-detect semver support for the selected registry
    let registry_url = get_registry();
    let supports = detect_supports_semver(&registry_url).await;
    let _ = SUPPORTS_SEMVER.set(supports);
}

pub fn get_registry() -> String {
    REGISTRY.get_sync()
}

pub fn set_legacy_peer_deps(value: Option<bool>) {
    LEGACY_PEER_DEPS.set(value);
}

pub async fn get_legacy_peer_deps() -> bool {
    LEGACY_PEER_DEPS.get().await
}

static OMIT: OnceLock<HashSet<OmitType>> = OnceLock::new();

pub fn set_omit(value: HashSet<OmitType>) {
    if !value.is_empty() {
        tracing::debug!("set omit: {:?}", value);
        let _ = OMIT.set(value);
    }
}

pub fn get_omit() -> HashSet<OmitType> {
    OMIT.get().cloned().unwrap_or_default()
}

// Manifest fetch concurrency configuration
static MANIFESTS_CONCURRENCY_LIMIT: LazyLock<ConfigValue<usize>> =
    LazyLock::new(|| ConfigValue::new("manifests-concurrency-limit", 64));

pub fn set_manifests_concurrency_limit(value: Option<usize>) {
    MANIFESTS_CONCURRENCY_LIMIT.set(value);
}

pub async fn get_manifests_concurrency_limit() -> usize {
    MANIFESTS_CONCURRENCY_LIMIT.get().await
}

pub fn get_manifests_concurrency_limit_sync() -> usize {
    MANIFESTS_CONCURRENCY_LIMIT.get_sync()
}

pub async fn set_cache_dir(cache_dir: Option<String>) {
    // Priority: CLI argument > UTOO_CACHE_DIR env > config > default
    let final_cache_dir = if let Some(dir) = cache_dir {
        Some(dir)
    } else if let Ok(env_dir) = std::env::var("UTOO_CACHE_DIR")
        && !env_dir.is_empty()
    {
        Some(env_dir)
    } else {
        // Read from config file if no CLI arg or env var
        Config::load(false)
            .await
            .ok()
            .and_then(|config| config.get("cache-dir").ok().flatten())
    };

    CACHE_DIR.set(final_cache_dir);
}

pub fn get_cache_dir() -> PathBuf {
    PathBuf::from(CACHE_DIR.get_sync())
}

// Semver support detection and caching
//
// Config key `supports-semver` is a TOML array of probe results. Each entry
// is the registry URL (positive → supports semver) or prefixed with "!"
// (negative → does NOT support):
//
//   [arrays]
//   supports-semver = [
//     "https://registry.npmmirror.com",
//     "!https://some-private.registry",
//   ]
//
// Multiple registries are cached simultaneously, so switching between them
// (e.g. `ut install --registry=xxx`) won't trigger a re-probe.
static SUPPORTS_SEMVER: OnceLock<bool> = OnceLock::new();

/// Probe a registry to detect whether it supports semver resolution.
///
/// 1. `is_npm_registry(url)` → return `false` immediately (known not to support)
/// 2. Search `arrays.supports-semver` for `"{url}"` (true) or `"!{url}"` (false)
/// 3. On cache miss, probe `GET $REGISTRY/utoo/%5E1` (5 s timeout)
/// 4. Append the result to the array for future runs
pub async fn detect_supports_semver(registry_url: &str) -> bool {
    use utoo_ruborist::registry::is_npm_registry;

    // Known: npm registry does not support semver queries
    if is_npm_registry(registry_url) {
        tracing::debug!("npm registry detected, skipping semver probe");
        return false;
    }

    // Check config cache (TOML array of "url" / "!url" entries)
    let mut config = Config::load(true).await.unwrap_or_default();
    let cached = config.get_array("supports-semver");

    if let Some(entries) = cached {
        for entry in entries {
            if entry == registry_url {
                tracing::debug!("Cache hit: supports-semver=true for {}", registry_url);
                return true;
            }
            if entry.strip_prefix('!') == Some(registry_url) {
                tracing::debug!("Cache hit: supports-semver=false for {}", registry_url);
                return false;
            }
        }
    }

    // Probe the registry
    let probe_url = format!("{}/utoo/%5E1", registry_url.trim_end_matches('/'));
    tracing::debug!("Probing semver support: {}", probe_url);

    let supports = async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        let resp = client
            .get(&probe_url)
            .header("Accept", "application/vnd.npm.install-v1+json")
            .send()
            .await?;
        tracing::debug!("Semver probe response: {} for {}", resp.status(), probe_url);
        Ok::<bool, reqwest::Error>(resp.status().is_success())
    }
    .await
    .unwrap_or_else(|e: reqwest::Error| {
        tracing::debug!("Semver probe failed: {}", e);
        false
    });

    // Append result to the cached array
    let new_entry = if supports {
        registry_url.to_string()
    } else {
        format!("!{}", registry_url)
    };
    let mut entries = config
        .get_array("supports-semver")
        .cloned()
        .unwrap_or_default();
    entries.push(new_entry);
    let _ = config.set_array("supports-semver", entries, true);

    tracing::debug!(
        "Detected supports-semver={} for {}",
        supports,
        registry_url
    );
    supports
}

pub fn get_supports_semver() -> Option<bool> {
    SUPPORTS_SEMVER.get().copied()
}

#[cfg(test)]
mod cache_dir_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_dir_default() {
        // Test default cache directory
        let default_cache = dirs::home_dir()
            .unwrap()
            .join(".cache/nm")
            .to_string_lossy()
            .to_string();

        // Create a fresh ConfigValue to simulate first access
        let cache_config = ConfigValue::new("cache-dir", default_cache.clone());
        let result = cache_config.get_sync();

        assert_eq!(result, default_cache);
    }

    #[test]
    fn test_config_value_parser_string() {
        let config = ConfigValue::new("test-key", "default-value".to_string());
        let parsed = config.parse_config_value("custom-value");
        assert_eq!(parsed, "custom-value");
    }

    #[test]
    fn test_config_value_parser_bool() {
        let config = ConfigValue::new("test-bool", false);
        assert!(config.parse_config_value("true"));
        assert!(config.parse_config_value("TRUE"));
        assert!(config.parse_config_value("True"));
        assert!(!config.parse_config_value("false"));
        assert!(!config.parse_config_value("anything"));
    }

    #[tokio::test]
    async fn test_cache_dir_from_config_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.toml");

        // Create a config file with cache-dir setting
        let custom_path = "/tmp/config-cache-test";
        let config_content = format!(
            r#"
[values]
cache-dir = "{}"
"#,
            custom_path
        );
        std::fs::write(&config_path, config_content)?;

        // Load config from file
        let config = Config::load_from_path(&config_path).await?;
        let cache_dir = config.get("cache-dir")?.unwrap();

        assert_eq!(cache_dir, custom_path);
        Ok(())
    }
}
