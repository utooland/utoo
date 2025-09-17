use super::logger::log_verbose;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::OnceLock;

pub type ConfigResult<T> = Result<T>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    values: HashMap<String, String>,
}

// global config path is ~/.utoo/config.toml
// local config path is .utoo/config.toml
impl Config {
    pub fn load(global: bool) -> ConfigResult<Self> {
        if global {
            return Self::load_from_path(&Self::global_config_path()?);
        }

        let mut config = Self::load_from_path(&Self::global_config_path()?)?;
        let local_path = Self::local_config_path()?;
        if local_path.exists() {
            let local_config = Self::load_from_path(&local_path)?;
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

    fn load_from_path(path: &Path) -> ConfigResult<Self> {
        if !path.exists() {
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
            log_verbose(&format!("set {}: {:?}", self.key, value));
            let _ = self.value.set(value);
        }
    }

    fn get(&self) -> T
    where
        Self: ConfigValueParser<T>,
    {
        if let Some(value) = self.value.get() {
            return value.clone();
        }

        // load from config - refactored for better readability
        let config_result = Config::load(false);
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

static REGISTRY: LazyLock<ConfigValue<String>> =
    LazyLock::new(|| ConfigValue::new("registry", "https://registry.npmmirror.com".to_string()));

static LEGACY_PEER_DEPS: LazyLock<ConfigValue<bool>> =
    LazyLock::new(|| ConfigValue::new("legacy-peer-deps", true));

static IS_NPM_REGISTRY: LazyLock<bool> = LazyLock::new(|| {
    let registry = REGISTRY.get();
    registry.contains("registry.npmjs.org") || registry.contains("registry.npmmirror.com")
});

pub fn set_registry(registry: Option<String>) {
    // Priority: CLI argument > UTOO_REGISTRY env > config
    let final_registry = registry.or_else(|| {
        std::env::var("UTOO_REGISTRY")
            .ok()
            .filter(|s| !s.is_empty())
    });
    REGISTRY.set(final_registry);
}

pub fn get_registry() -> String {
    REGISTRY.get()
}

pub fn set_legacy_peer_deps(value: Option<bool>) {
    LEGACY_PEER_DEPS.set(value);
}

pub fn get_legacy_peer_deps() -> bool {
    LEGACY_PEER_DEPS.get()
}

pub fn get_registry_support_abbr() -> bool {
    !*IS_NPM_REGISTRY
}

pub fn get_registry_support_semver() -> bool {
    !*IS_NPM_REGISTRY
}
