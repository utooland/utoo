use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use utoo_ruborist::spec::Catalogs;

pub use super::cli_enum::ConfigScope;

pub type ConfigResult<T> = Result<T>;

/// Cached merged config (global + local). Set on first `Config::load(ConfigScope::Local)`.
static MERGED_CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    values: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    arrays: HashMap<String, Vec<String>>,
    /// Default catalog: `[catalog]` section.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    catalog: HashMap<String, String>,
    /// Named catalogs: `[catalogs.<name>]` sections.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    catalogs: HashMap<String, HashMap<String, String>>,
    /// Install-time script allowlist: `[allowScripts]` section, `name -> bool`
    /// (`true` allows the package to run install scripts, `false` denies it).
    /// Named `allowScripts` to match npm's `package.json` key.
    #[serde(
        rename = "allowScripts",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    allow_scripts: HashMap<String, bool>,
}

// global config path is ~/.utoo/config.toml
// local config path is .utoo.toml
impl Config {
    pub async fn load(scope: ConfigScope) -> ConfigResult<Self> {
        if scope == ConfigScope::Global {
            return Self::load_from_path(&Self::global_config_path()?).await;
        }

        // Return cached merged config if available
        if let Some(config) = MERGED_CONFIG.get() {
            return Ok(config.clone());
        }

        let mut config = Self::load_from_path(&Self::global_config_path()?).await?;

        let local_config = {
            let local_path = Self::local_config_path()?;
            if crate::fs::try_exists(&local_path).await? {
                Some(Self::load_from_path(&local_path).await?)
            } else {
                None
            }
        };
        if let Some(local) = local_config {
            config.values.extend(local.values);
            config.arrays.extend(local.arrays);
            // Local `allowScripts` entries override global per package name; a
            // global deny that the project re-allows is intentional (the project
            // owner outranks the machine default). Cross-source deny precedence
            // for the *effective* policy is applied in `script_policy`.
            config.allow_scripts.extend(local.allow_scripts);
            // Catalogs are project-local only; take them from the local config
            config.catalog = local.catalog;
            config.catalogs = local.catalogs;
        }

        let _ = MERGED_CONFIG.set(config.clone());
        Ok(config)
    }

    /// Load the global and project-local configs **separately** (not merged),
    /// returning `(global, local)` where `local` is `None` when no `.utoo.toml`
    /// exists. Callers that need cross-source precedence — e.g. the install
    /// script policy, where a team-level (global) deny must survive a
    /// project-level allow — layer these in order instead of consuming the
    /// flattened [`Self::load`] view (whose per-key `extend` would let a local
    /// allow erase a global deny). A genuine parse error propagates.
    pub async fn load_layers() -> ConfigResult<(Self, Option<Self>)> {
        let global = Self::load_from_path(&Self::global_config_path()?).await?;
        let local_path = Self::local_config_path()?;
        let local = if crate::fs::try_exists(&local_path).await? {
            Some(Self::load_from_path(&local_path).await?)
        } else {
            None
        };
        Ok((global, local))
    }

    pub fn set(&mut self, key: &str, value: String, scope: ConfigScope) -> ConfigResult<()> {
        self.values.insert(key.to_string(), value);
        self.save(scope)
    }

    pub fn get(&self, key: &str) -> ConfigResult<Option<String>> {
        Ok(self.values.get(key).cloned())
    }

    pub fn delete(&mut self, key: &str, scope: ConfigScope) -> ConfigResult<()> {
        self.values.remove(key);
        self.save(scope)
    }

    pub fn get_array(&self, key: &str) -> Option<&[String]> {
        self.arrays.get(key).map(|v| v.as_slice())
    }

    /// The `[allowScripts]` table (`name -> allow`), used to build the
    /// install-time script policy. See [`crate::util::script_policy`].
    pub fn allow_scripts(&self) -> &HashMap<String, bool> {
        &self.allow_scripts
    }

    pub fn set_array(
        &mut self,
        key: &str,
        value: Vec<String>,
        scope: ConfigScope,
    ) -> ConfigResult<()> {
        self.arrays.insert(key.to_string(), value);
        self.save(scope)
    }

    /// Build a `Catalogs` map from the parsed `[catalog]` and `[catalogs.*]` sections.
    ///
    /// The default catalog (`[catalog]`) is stored under both `""` and `"default"`
    /// so that `catalog:` and `catalog:default` both resolve without extra normalization.
    pub fn catalogs(&self) -> Catalogs {
        let mut result = self.catalogs.clone();
        if !self.catalog.is_empty() {
            result.insert(String::new(), self.catalog.clone());
        }
        // Duplicate default catalog under "default" key for direct lookup
        if let Some(default) = result.get("").cloned() {
            result.entry("default".to_string()).or_insert(default);
        }
        result
    }

    pub(crate) async fn load_from_path(path: &Path) -> ConfigResult<Self> {
        match crate::fs::read_to_string(path).await {
            Ok(content) => Ok(toml::from_str(&content)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, scope: ConfigScope) -> ConfigResult<()> {
        let path = match scope {
            ConfigScope::Global => Self::global_config_path()?,
            ConfigScope::Local => Self::local_config_path()?,
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

        // Ensure merged config is loaded and cached
        if MERGED_CONFIG.get().is_none() {
            let _ = Config::load(ConfigScope::Local).await; // populates MERGED_CONFIG
        }

        // Read from cached merged config (no clone of Config itself)
        if let Some(config) = MERGED_CONFIG.get()
            && let Ok(Some(value)) = config.get(self.key)
        {
            let parsed_value = self.parse_config_value(&value);
            let _ = self.value.set(parsed_value.clone());
            return parsed_value;
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
    use std::sync::Mutex;

    /// Serialize tests that override HOME to avoid races.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    /// Run an async closure with HOME pointed at a temp dir, so
    /// Config::load/save(global=true) never touches the real config.
    fn with_temp_home(f: impl FnOnce()) {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let prev = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", dir.path()) };

        f();

        match prev {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    #[test]
    fn test_delete_removes_key_and_persists() {
        with_temp_home(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut config = Config::load(ConfigScope::Global).await.unwrap();
                config
                    .set("foo", "bar".into(), ConfigScope::Global)
                    .unwrap();
                config
                    .set("baz", "qux".into(), ConfigScope::Global)
                    .unwrap();

                // call the actual delete method
                config.delete("foo", ConfigScope::Global).unwrap();

                // in-memory: foo gone, baz kept
                assert_eq!(config.get("foo").unwrap(), None);
                assert_eq!(config.get("baz").unwrap(), Some("qux".into()));

                // reload from disk: still gone
                let reloaded = Config::load(ConfigScope::Global).await.unwrap();
                assert_eq!(reloaded.get("foo").unwrap(), None);
                assert_eq!(reloaded.get("baz").unwrap(), Some("qux".into()));
            });
        });
    }

    #[test]
    fn test_delete_nonexistent_key_is_noop() {
        with_temp_home(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut config = Config::load(ConfigScope::Global).await.unwrap();
                config
                    .set("keep", "yes".into(), ConfigScope::Global)
                    .unwrap();

                // deleting a key that doesn't exist should not error
                config.delete("nope", ConfigScope::Global).unwrap();

                assert_eq!(config.get("keep").unwrap(), Some("yes".into()));
            });
        });
    }

    #[test]
    fn test_catalogs_default_and_named() {
        let config: Config = toml::from_str(
            r#"
[catalog]
lodash = "^4.17.21"
react = "^18.0.0"

[catalogs.legacy]
path-to-regexp = "^1.9.0"
"#,
        )
        .unwrap();

        let catalogs = config.catalogs();
        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("lodash"), Some(&"^4.17.21".to_string()));
        assert_eq!(default.get("react"), Some(&"^18.0.0".to_string()));

        let legacy = catalogs.get("legacy").unwrap();
        assert_eq!(legacy.get("path-to-regexp"), Some(&"^1.9.0".to_string()));
    }

    #[test]
    fn test_catalogs_empty() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.catalogs().is_empty());
    }

    #[test]
    fn test_allow_scripts_table_parses() {
        let config: Config = toml::from_str(
            r#"
[allowScripts]
"esbuild@0.25.5" = true
sharp = true
"telemetry-pkg" = false
"#,
        )
        .unwrap();

        let allow = config.allow_scripts();
        assert_eq!(allow.get("esbuild@0.25.5"), Some(&true));
        assert_eq!(allow.get("sharp"), Some(&true));
        assert_eq!(allow.get("telemetry-pkg"), Some(&false));
    }

    #[test]
    fn test_catalogs_coexists_with_config_values() {
        let config: Config = toml::from_str(
            r#"
[values]
registry = "https://registry.npmmirror.com"

[catalog]
lodash = "^4.17.21"
"#,
        )
        .unwrap();

        assert_eq!(
            config.get("registry").unwrap(),
            Some("https://registry.npmmirror.com".to_string())
        );
        let catalogs = config.catalogs();
        assert_eq!(
            catalogs.get("").unwrap().get("lodash"),
            Some(&"^4.17.21".to_string())
        );
    }
}
