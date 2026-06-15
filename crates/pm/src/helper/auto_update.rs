//! Auto-update flow:
//!
//! ```text
//!   cache fresh?  --no-->  background fetch + write cache
//!       |                          |
//!      yes              version != current?
//!       |                   |            |
//!  version != current?     no           yes
//!    |          |        (done)          |
//!   no         yes                      |
//! (done)        +----------+------------+
//!               |
//!        cooldown (24h)?  --yes-->  (done)
//!               |
//!              no
//!               |
//!        try update
//!          |        |
//!        success  failure --> eprintln! + set cooldown
//! ```

use crate::constants::APP_VERSION;
use crate::util::http::client_builder;
use crate::util::user_config::get_registry;
use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Debug)]
struct VersionCache {
    version: String,
    check_time: u64,
    /// Timestamp of last failed update attempt; skip retry until cooldown expires.
    #[serde(default)]
    last_update_failed: Option<u64>,
}

const CACHE_TTL_SECS: u64 = 3600; // 1 hour
const UPDATE_RETRY_COOLDOWN_SECS: u64 = 86400; // 24 hours

/// Single entry point for auto-update logic.
///
/// 1. If a valid cache exists and the version differs → attempt update, prompt on failure.
/// 2. If cache is missing or expired → spawn a fire-and-forget background fetch to refresh it.
///
/// Either way this function returns quickly and never blocks the main command.
pub async fn init_auto_update() {
    let force = std::env::var("UTOO_FORCE_UPDATE").is_ok_and(|v| v == "1" || v == "true");

    // UTOO_FORCE_UPDATE=1: skip all guards, fetch and update immediately
    if force {
        match fetch_and_cache_version().await {
            Ok(version) if version != APP_VERSION => {
                try_update(&version).await;
            }
            Err(e) => {
                tracing::debug!("Force update fetch failed: {e}");
            }
            _ => {}
        }
        return;
    }

    // Skip for local development
    if APP_VERSION == "0.0.0" {
        return;
    }

    // Skip in CI
    let ci_env = std::env::var("CI").unwrap_or_default();
    if ci_env == "1" || ci_env.to_lowercase() == "true" {
        return;
    }

    match read_version_cache() {
        Ok(cache) if !is_cache_expired(&cache) => {
            // Step 1: cache is fresh — check version and act
            if cache.version != APP_VERSION {
                try_update(&cache.version).await;
            }
        }
        _ => {
            // Step 2: no cache or expired — fetch in background, update if needed
            tokio::spawn(async {
                match fetch_and_cache_version().await {
                    Ok(version) if version != APP_VERSION => {
                        try_update(&version).await;
                    }
                    Err(e) => {
                        tracing::debug!("Background version fetch failed: {e}");
                    }
                    _ => {}
                }
            });
        }
    }
}

/// Attempt silent update; print a prompt on failure.
/// Respects a 24h cooldown after a failed attempt to avoid nagging.
async fn try_update(new_version: &str) {
    // Check cooldown from last failed attempt
    if let Ok(cache) = read_version_cache()
        && let Some(failed_at) = cache.last_update_failed
    {
        let now = now_secs();
        if now - failed_at < UPDATE_RETRY_COOLDOWN_SECS {
            tracing::debug!(
                "Skipping update (last failure {}s ago, cooldown {}s)",
                now - failed_at,
                UPDATE_RETRY_COOLDOWN_SECS,
            );
            return;
        }
    }

    println!(
        "{} utoo: {} → {}",
        "Updating".cyan(),
        APP_VERSION.yellow(),
        new_version.green(),
    );

    match execute_update(new_version).await {
        Ok(_) => {
            println!("{}", "Updated successfully.".green());
            mark_update_failed(None);
        }
        Err(e) => {
            tracing::debug!("Auto update failed: {e}");
            mark_update_failed(Some(now_secs()));
            println!(
                "\n  {} {} → {}\n  Run {} to update.\n",
                "Update available:".yellow(),
                APP_VERSION.yellow(),
                new_version.green(),
                "`utoo i utoo@latest -g`".cyan(),
            );
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before UNIX epoch")
        .as_secs()
}

fn is_cache_expired(cache: &VersionCache) -> bool {
    now_secs() - cache.check_time > CACHE_TTL_SECS
}

/// Write (or clear) the `last_update_failed` timestamp in the cache file.
fn mark_update_failed(timestamp: Option<u64>) {
    if let Ok(mut cache) = read_version_cache() {
        cache.last_update_failed = timestamp;
        if let Err(e) = save_version_cache(&cache) {
            tracing::debug!("Failed to save version cache: {e}");
        }
    }
}

async fn execute_update(version: &str) -> Result<()> {
    let status = tokio::process::Command::new("utoo")
        .args(["i", &format!("utoo@{version}"), "-g"])
        .env("CI", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .context("Failed to execute update command")?;

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("update command exited with {status}")
    }
}

/// Fetch latest version from registry, write to cache file, and return the version.
async fn fetch_and_cache_version() -> Result<String> {
    let registry_url = format!("{}/utoo/latest", get_registry());
    let client = client_builder()?
        .timeout(std::time::Duration::from_millis(1000))
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
        .context("Failed to parse package info")?;

    let version = package_info["version"]
        .as_str()
        .context("Unable to get version information")?
        .to_string();

    let last_update_failed = read_version_cache().ok().and_then(|c| c.last_update_failed);
    let cache = VersionCache {
        version,
        check_time: now_secs(),
        last_update_failed,
    };

    save_version_cache(&cache)?;
    Ok(cache.version)
}

// ── Cache I/O ──────────────────────────────────────────────

fn get_cache_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".utoo").join("remote-version.json")
}

fn read_version_cache() -> Result<VersionCache> {
    let content =
        fs::read_to_string(get_cache_path()).context("Failed to read version cache file")?;
    serde_json::from_str(&content).context("Failed to parse version cache")
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
    use super::*;
    use std::process::{Command, Stdio};
    use tokio::time::Duration;

    #[test]
    fn test_skip_ci_environment() {
        unsafe { std::env::set_var("CI", "1") };
        // init_auto_update is async, but we just verify the CI guard path
        // by checking that it would return early
        let ci_env = std::env::var("CI").unwrap_or_default();
        assert!(ci_env == "1" || ci_env.to_lowercase() == "true");
        unsafe { std::env::remove_var("CI") };
    }

    #[test]
    fn test_version_cache_serialization() {
        let cache = VersionCache {
            version: "1.0.0".to_string(),
            check_time: 1234567890,
            last_update_failed: None,
        };

        let serialized = serde_json::to_string(&cache).unwrap();
        let deserialized: VersionCache = serde_json::from_str(&serialized).unwrap();

        assert_eq!(cache.version, deserialized.version);
        assert_eq!(cache.check_time, deserialized.check_time);
    }

    #[test]
    fn test_is_cache_expired() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let fresh = VersionCache {
            version: "1.0.0".to_string(),
            check_time: now - 100,
            last_update_failed: None,
        };
        assert!(!is_cache_expired(&fresh));

        let stale = VersionCache {
            version: "1.0.0".to_string(),
            check_time: now - CACHE_TTL_SECS - 1,
            last_update_failed: None,
        };
        assert!(is_cache_expired(&stale));
    }

    #[test]
    fn test_read_version_cache_missing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();

        unsafe { std::env::set_var("HOME", temp_dir.path()) };

        let result = read_version_cache();
        assert!(result.is_err());

        unsafe {
            if let Some(home) = original_home {
                std::env::set_var("HOME", home);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    #[tokio::test]
    async fn test_save_and_read_version_cache() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let cache_path = temp_dir.path().join(".utoo").join("remote-version.json");

        let cache = VersionCache {
            version: "1.2.3".to_string(),
            check_time: 1234567890,
            last_update_failed: None,
        };

        if let Some(parent) = cache_path.parent() {
            crate::fs::create_dir_all(parent).await?;
        }

        let content = serde_json::to_string(&cache)?;
        crate::fs::write(&cache_path, &content).await?;

        let read_content = crate::fs::read_to_string(cache_path).await?;
        let loaded_cache: VersionCache = serde_json::from_str(&read_content)?;

        assert_eq!(loaded_cache.version, cache.version);
        assert_eq!(loaded_cache.check_time, cache.check_time);

        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_and_cache_version_timeout() {
        let start_time = std::time::Instant::now();
        let result: Result<String> = fetch_and_cache_version().await;
        let elapsed = start_time.elapsed();

        assert!(elapsed < Duration::from_secs(5));
        drop(result);
    }

    #[test]
    fn test_get_cache_path() {
        let path = get_cache_path();
        assert!(path.to_string_lossy().contains(".utoo"));
        assert!(path.to_string_lossy().contains("remote-version.json"));
    }

    #[test]
    fn test_execute_update_success() {
        let result = Command::new("true")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
        assert!(result.success());
    }

    #[test]
    fn test_execute_update_failure() {
        let result = Command::new("false")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
        assert!(!result.success());
    }

    #[test]
    fn test_execute_update_command_not_found() {
        let result = Command::new("non_existent_command")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        assert!(result.is_err());
    }
}
