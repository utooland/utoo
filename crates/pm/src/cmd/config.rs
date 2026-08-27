use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::cli::ConfigCommands;
use crate::error::CliError;
use crate::model::cli_output::{ConfigGetResult, ConfigListResult, ConfigSetResult};
use crate::util::cli_enum::ConfigScope;
use crate::util::config_file::Config;
use crate::util::presenter::emit;

/// Entry point for the `config` subcommand.
pub async fn run(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Set { key, value, global } => {
            handle_config_set(key, value, global.into()).await
        }
        ConfigCommands::Get {
            key,
            global,
            override_values,
        } => handle_config_get(key, global.into(), override_values).await,
        ConfigCommands::List { global } => handle_config_list(global.into()).await,
    }
}

// Parse key val manually
fn parse_key_val(s: &str) -> Result<(String, String)> {
    let pos = s
        .find('=')
        .ok_or_else(|| anyhow!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

pub async fn handle_config_set(key: String, value: String, scope: ConfigScope) -> Result<()> {
    let mut config = Config::load(scope).await?;
    config.set(&key, value.clone(), scope)?;
    let label = if scope == ConfigScope::Global {
        "global"
    } else {
        "local"
    };
    let output = ConfigSetResult {
        values: BTreeMap::from([(key.clone(), Value::String(value.clone()))]),
        scope: label.to_string(),
    };
    emit("config", &output, || {
        println!("Successfully set {key} ({label})");
        Ok(())
    })
}

pub async fn handle_config_get(
    key: String,
    scope: ConfigScope,
    override_values: Vec<String>,
) -> Result<()> {
    let overrides: HashMap<String, String> = override_values
        .iter()
        .filter_map(|arg| arg.strip_prefix("--").and_then(|s| parse_key_val(s).ok()))
        .collect();

    if let Some(value) = overrides.get(&key) {
        emit_config_get(&key, value, "override")?;
    } else {
        let resolved = match scope {
            ConfigScope::Global => Config::load(ConfigScope::Global)
                .await?
                .get(&key)?
                .map(|value| (value, "global")),
            ConfigScope::Local => {
                let (global, local) = Config::load_levels().await?;
                match local
                    .as_ref()
                    .map(|config| config.get(&key))
                    .transpose()?
                    .flatten()
                {
                    Some(value) => Some((value, "local")),
                    None => global.get(&key)?.map(|value| (value, "global")),
                }
            }
        };
        match resolved {
            Some((value, source)) => emit_config_get(&key, &value, source)?,
            None => return Err(CliError::not_found(format!("Key '{key}' not found")).into()),
        };
    }
    Ok(())
}

pub async fn handle_config_list(scope: ConfigScope) -> Result<()> {
    let config = Config::load(scope).await?;
    let config_path = match scope {
        ConfigScope::Global => config.get_global_config_path()?,
        ConfigScope::Local => config.get_local_config_path()?,
    };

    let mut values: BTreeMap<String, Value> = config
        .list()?
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    values.extend(
        config
            .list_arrays()
            .map(|(key, values)| (key.clone(), serde_json::json!(values))),
    );
    let output = ConfigListResult {
        path: config_path.display().to_string(),
        scope: scope_label(scope).to_string(),
        values,
    };
    emit("config", &output, || {
        println!("Configuration file: {}", config_path.display());
        println!();
        let mut values = config.list()?.collect::<Vec<_>>();
        values.sort_unstable_by_key(|(key, _)| *key);
        for (key, value) in values {
            println!("{key} = {value}");
        }

        let mut arrays = config.list_arrays().collect::<Vec<_>>();
        arrays.sort_unstable_by_key(|(key, _)| *key);
        for (key, values) in arrays {
            println!("{key} = [{}]", values.join(", "));
        }
        Ok(())
    })
}

fn emit_config_get(key: &str, value: &str, source: &str) -> Result<()> {
    let output = ConfigGetResult {
        values: BTreeMap::from([(key.to_string(), Value::String(value.to_string()))]),
        source: source.to_string(),
    };
    emit("config", &output, || {
        println!("{value}");
        Ok(())
    })
}

fn scope_label(scope: ConfigScope) -> &'static str {
    match scope {
        ConfigScope::Global => "global",
        ConfigScope::Local => "local",
    }
}
