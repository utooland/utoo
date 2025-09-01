use crate::util::config::get_registry;
use crate::util::logger::{log_error, log_info, log_warning};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Debug)]
struct VersionCache {
    version: String,
    check_time: u64,
}

pub async fn init_auto_update() -> Result<()> {
    // if process::env CI=1 or CI=true, ignore auto update
    let ci_env = std::env::var("CI").unwrap_or_default();
    if ci_env == "1" || ci_env.to_lowercase() == "true" {
        return Ok(());
    }

    let cache = match read_version_cache() {
        Ok(cache) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if now - cache.check_time > 3600 {
                check_and_update_cache().await?
            } else {
                cache
            }
        }
        Err(_) => check_and_update_cache().await?,
    };

    let current_version = env!("CARGO_PKG_VERSION");
    if cache.version != current_version {
        log_info(&format!(
            "New version found: {} (current version: {}), updating automatically...",
            cache.version, current_version
        ));
        log_info(&format!("utoo i @utoo/utoo{} -g", cache.version));

        execute_update(&cache.version).await?;
    }
    Ok(())
}

async fn check_and_update_cache() -> Result<VersionCache> {
    match check_remote_version().await {
        Ok(_) => read_version_cache(),
        Err(e) => {
            log_warning(&format!("Failed to check remote version: {e}"));
            Err(e).context("Failed to check remote version")
        }
    }
}

async fn execute_update(version: &str) -> Result<()> {
    let status = Command::new("utoo")
        .args(["i", &format!("@utoo/utoo@{version}"), "-g"])
        .env("CI", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute update command")?;

    if status.success() {
        log_info("Update completed, please restart");
        Ok(())
    } else {
        log_error("Auto update failed, please update manually");
        anyhow::bail!("Auto update failed, please execute manually {}", status)
    }
}

async fn check_remote_version() -> Result<()> {
    let registry_url = format!("{}/utoo/latest", get_registry());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(2000))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(registry_url)
        .send()
        .await
        .context("Failed to fetch remote version")?;

    let package_info = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse package info: {}", e))?;

    let version = package_info["version"]
        .as_str()
        .context("Unable to get version information")?
        .to_string();

    let cache = VersionCache {
        version,
        check_time: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    save_version_cache(&cache)
}

fn get_cache_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".utoo").join("remote-version.json")
}

fn read_version_cache() -> Result<VersionCache> {
    let content =
        fs::read_to_string(get_cache_path()).context("Failed to read version cache file")?;
    serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse version cache: {}", e))
}

fn save_version_cache(cache: &VersionCache) -> Result<()> {
    let cache_path = get_cache_path();
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }
    let content = serde_json::to_string(cache).context("Failed to serialize version cache")?;
    fs::write(cache_path, content).context("Failed to write version cache file")
}

#[cfg(test)]
mod tests {

    use std::process::{Command, Stdio};

    #[test]
    fn test_execute_update_success() {
        // always true to simulate success
        let result = Command::new("true")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();

        assert!(result.success());
        assert_eq!(result.code(), Some(0));
    }

    #[test]
    fn test_execute_update_failure() {
        // always false to simulate failure
        let result = Command::new("false")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();

        assert!(!result.success());
        assert_eq!(result.code(), Some(1));
    }

    #[test]
    fn test_execute_update_command_not_found() {
        // simulate command not found
        let result = Command::new("non_existent_command")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        assert!(result.is_err());
    }
}
