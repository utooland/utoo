use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use super::save_type::OmitType;

pub type ConfigResult<T> = Result<T>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    values: HashMap<String, String>,
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
        if tokio::fs::try_exists(&local_path).await? {
            let local_config = Self::load_from_path(&local_path).await?;
            config.values.extend(local_config.values);
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

    async fn load_from_path(path: &Path) -> ConfigResult<Self> {
        if !tokio::fs::try_exists(path).await? {
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
    LazyLock::new(|| ConfigValue::new("registry", "https://registry.npmmirror.com".to_string()));

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

static IS_NPM_REGISTRY: OnceLock<bool> = OnceLock::new();

fn is_npm_registry_url(url: &str) -> bool {
    url.contains("registry.npmjs.org") || url.contains("registry.npmjs.com")
}

pub async fn set_registry(registry: Option<String>) {
    // Priority: CLI argument > UTOO_REGISTRY env > config > default
    let final_registry = if let Some(reg) = registry {
        Some(reg)
    } else if let Ok(env_reg) = std::env::var("UTOO_REGISTRY")
        && !env_reg.is_empty()
    {
        Some(env_reg)
    } else {
        // Read from config file if no CLI arg or env var
        Config::load(false)
            .await
            .ok()
            .and_then(|config| config.get("registry").ok().flatten())
    };

    // Determine if this is npm registry and set the global flag
    let registry_url = final_registry
        .as_deref()
        .unwrap_or("https://registry.npmmirror.com");
    let _ = IS_NPM_REGISTRY.set(is_npm_registry_url(registry_url));

    REGISTRY.set(final_registry);
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

fn ensure_is_npm_registry_initialized() -> bool {
    *IS_NPM_REGISTRY.get_or_init(|| false)
}

pub fn get_registry_support_abbr() -> bool {
    !ensure_is_npm_registry_initialized()
}

pub fn get_registry_support_semver() -> bool {
    !ensure_is_npm_registry_initialized()
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

// Preload concurrency configuration
static PRELOAD_MANIFESTS_LIMIT: LazyLock<ConfigValue<usize>> =
    LazyLock::new(|| ConfigValue::new("preload-manifests-limit", 64));

static PRELOAD_DOWNLOADS_LIMIT: LazyLock<ConfigValue<usize>> =
    LazyLock::new(|| ConfigValue::new("preload-downloads-limit", 100));

pub fn set_preload_manifests_limit(value: Option<usize>) {
    PRELOAD_MANIFESTS_LIMIT.set(value);
}

pub async fn get_preload_manifests_limit() -> usize {
    PRELOAD_MANIFESTS_LIMIT.get().await
}

pub fn set_preload_downloads_limit(value: Option<usize>) {
    PRELOAD_DOWNLOADS_LIMIT.set(value);
}

pub async fn get_preload_downloads_limit() -> usize {
    PRELOAD_DOWNLOADS_LIMIT.get().await
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
