use std::collections::HashMap;

use anyhow::{Result, anyhow};

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
    config.set(&key, value, scope)?;
    let label = if scope == ConfigScope::Global {
        "global"
    } else {
        "local"
    };
    println!("Successfully set {key} ({label})");
    Ok(())
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
        println!("{value}");
    } else {
        let config = Config::load(scope).await?;
        match config.get(&key)? {
            Some(value) => println!("{value}"),
            None => {
                eprintln!("Key '{key}' not found");
                std::process::exit(1);
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

    println!("Configuration file: {}", config_path.display());
    println!();

    for (key, value) in config.list()? {
        println!("{key} = {value}");
    }
    for (key, values) in config.list_arrays() {
        println!("{key} = [{}]", values.join(", "));
    }
    Ok(())
}
