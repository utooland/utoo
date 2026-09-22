use crate::util::config_file::{Config, ConfigResult};

pub struct ConfigService {
    config: Config,
}

impl ConfigService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn get_available_commands(&self) -> ConfigResult<Vec<(String, String)>> {
        Ok(self
            .config
            .list()?
            .filter(|(key, _)| key.ends_with(".cmd"))
            .filter_map(|(key, _)| {
                let command = key.strip_suffix(".cmd")?;
                let alias = self.config.get(key).ok()??;
                Some((command.to_string(), alias))
            })
            .collect::<Vec<_>>())
    }

    pub fn get_available_cmd(&self, command: &str) -> ConfigResult<Option<String>> {
        // Check if there's a custom command configured for this command name
        self.config.get(&format!("{command}.cmd"))
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}
