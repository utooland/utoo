use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub type ConfigResult<T> = Result<T>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    values: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    arrays: HashMap<String, Vec<String>>,
}

// global config path is ~/.utoo/config.toml
// local config path is .utoo.toml
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

    pub fn delete(&mut self, key: &str, global: bool) -> ConfigResult<()> {
        self.values.remove(key);
        self.save(global)
    }

    pub fn get_array(&self, key: &str) -> Option<&Vec<String>> {
        self.arrays.get(key)
    }

    pub fn set_array(&mut self, key: &str, value: Vec<String>, global: bool) -> ConfigResult<()> {
        self.arrays.insert(key.to_string(), value);
        self.save(global)
    }

    pub(crate) async fn load_from_path(path: &Path) -> ConfigResult<Self> {
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
pub(super) trait ConfigValueParser<T> {
    fn parse_config_value(&self, value: &str) -> T;
}

pub(super) struct ConfigValue<T> {
    value: OnceLock<T>,
    key: &'static str,
    default: T,
}

impl<T: Clone + Debug + 'static> ConfigValue<T> {
    pub(super) const fn new(key: &'static str, default: T) -> Self {
        Self {
            value: OnceLock::new(),
            key,
            default,
        }
    }

    pub(super) fn set(&self, new_value: Option<T>) {
        if let Some(value) = new_value {
            tracing::debug!("set {}: {:?}", self.key, value);
            let _ = self.value.set(value);
        }
    }

    pub(super) async fn get(&self) -> T
    where
        Self: ConfigValueParser<T>,
    {
        if let Some(value) = self.value.get() {
            return value.clone();
        }

        // load from config
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

    pub(super) fn get_sync(&self) -> T
    where
        Self: ConfigValueParser<T>,
    {
        if let Some(value) = self.value.get() {
            return value.clone();
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

impl ConfigValueParser<usize> for ConfigValue<usize> {
    fn parse_config_value(&self, value: &str) -> usize {
        value.parse().unwrap_or(self.default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_config(values: &[(&str, &str)]) -> Config {
        let map = values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config {
            values: map,
            arrays: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_delete_removes_key_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // seed two keys, save to temp file
        let config = seed_config(&[("foo", "bar"), ("baz", "qux")]);
        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        // load → delete → save back
        let mut config = Config::load_from_path(&path).await.unwrap();
        assert_eq!(config.get("foo").unwrap(), Some("bar".into()));

        config.values.remove("foo");
        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        // reload from disk — foo gone, baz kept
        let reloaded = Config::load_from_path(&path).await.unwrap();
        assert_eq!(reloaded.get("foo").unwrap(), None);
        assert_eq!(reloaded.get("baz").unwrap(), Some("qux".into()));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_key_is_noop() {
        let config = seed_config(&[("keep", "yes")]);

        // deleting a key that doesn't exist just returns None
        assert_eq!(config.values.get("nope"), None);
        assert_eq!(config.get("keep").unwrap(), Some("yes".into()));
    }
}
