use super::logger::log_verbose;
use std::fmt::Debug;
use std::sync::LazyLock;
use std::sync::OnceLock;
use utoo_core::config::Config;

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

        // load from utoo-core config
        if let Ok(config) = Config::load(false)
            && let Ok(Some(value)) = config.get(self.key)
        {
            let parsed_value = self.parse_config_value(&value);
            let _ = self.value.set(parsed_value.clone());
            return parsed_value;
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
