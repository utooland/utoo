use crate::util::config::Config;
use anyhow::{Result, anyhow};
use std::collections::HashMap;

// Parse key val manually
fn parse_key_val(s: &str) -> Result<(String, String)> {
    let pos = s
        .find('=')
        .ok_or_else(|| anyhow!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

pub async fn handle_config_set(key: String, value: String, global: bool) -> Result<()> {
    let mut config = Config::load(global).await?;
    config.set(&key, value, global)?;
    println!("Successfully set {key} (global: {global})");
    Ok(())
}

pub async fn handle_config_get(
    key: String,
    global: bool,
    override_values: Vec<String>,
) -> Result<()> {
    let overrides: HashMap<String, String> = override_values
        .iter()
        .filter_map(|arg| arg.strip_prefix("--").and_then(|s| parse_key_val(s).ok()))
        .collect();

    if let Some(value) = overrides.get(&key) {
        println!("{value}");
    } else {
        let config = Config::load(global).await?;
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

pub async fn handle_config_list(global: bool) -> Result<()> {
    let config = Config::load(global).await?;
    let config_path = if global {
        config.get_global_config_path()?
    } else {
        config.get_local_config_path()?
    };

    println!("Configuration file: {}", config_path.display());
    println!();

    for (key, value) in config.list()? {
        println!("{key} = {value}");
    }
    Ok(())
}
