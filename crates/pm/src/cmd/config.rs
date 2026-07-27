use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, anyhow};
use serde::Serialize;

use crate::cli::ConfigCommands;
use crate::util::cli_enum::ConfigScope;
use crate::util::config_file::Config;

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
    let output = ConfigSetOutput {
        key: &key,
        value: &value,
        scope: label,
    };
    crate::util::presenter::emit("config set", &output, || {
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
        let config = Config::load(scope).await?;
        match config.get(&key)? {
            Some(value) => emit_config_get(&key, &value, scope_label(scope))?,
            None => {
                return Err(
                    crate::error::CliError::not_found(format!("Key '{key}' not found")).into(),
                );
            }
        }
    }
    Ok(())
}

pub async fn handle_config_list(scope: ConfigScope) -> Result<()> {
    let config = Config::load(scope).await?;
    let config_path = match scope {
        ConfigScope::Global => config.get_global_config_path()?,
        ConfigScope::Local => config.get_local_config_path()?,
    };

    let values: BTreeMap<&str, &str> = config
        .list()?
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let arrays: BTreeMap<&str, &Vec<String>> = config
        .list_arrays()
        .map(|(key, values)| (key.as_str(), values))
        .collect();
    let output = ConfigListOutput {
        path: config_path.display().to_string(),
        scope: scope_label(scope),
        values,
        arrays,
    };
    crate::util::presenter::emit("config list", &output, || {
        println!("Configuration file: {}", config_path.display());
        println!();
        for (key, value) in config.list()? {
            println!("{key} = {value}");
        }
        for (key, values) in config.list_arrays() {
            println!("{key} = [{}]", values.join(", "));
        }
        Ok(())
    })
}

fn emit_config_get(key: &str, value: &str, source: &str) -> Result<()> {
    let output = ConfigGetOutput { key, value, source };
    crate::util::presenter::emit("config get", &output, || {
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

#[derive(Serialize)]
struct ConfigSetOutput<'a> {
    key: &'a str,
    value: &'a str,
    scope: &'a str,
}

#[derive(Serialize)]
struct ConfigGetOutput<'a> {
    key: &'a str,
    value: &'a str,
    source: &'a str,
}

#[derive(Serialize)]
struct ConfigListOutput<'a> {
    path: String,
    scope: &'a str,
    values: BTreeMap<&'a str, &'a str>,
    arrays: BTreeMap<&'a str, &'a Vec<String>>,
}
