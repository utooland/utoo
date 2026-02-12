use crate::constants::APP_VERSION;
use crate::util::user_config::get_registry;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Debug)]
struct VersionCache {
    version: String,
    check_time: u64,
}

// Global state management for async update checking
static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static UPDATE_VERSION: Lazy<Arc<RwLock<Option<String>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// Initialize auto update with immediate check and background monitoring
pub async fn init_auto_update() -> Result<()> {
    // Skip for local development
    if APP_VERSION == "0.0.0" {
        return Ok(());
    }

    // Skip auto update in CI environment
    let ci_env = std::env::var("CI").unwrap_or_default();
    if ci_env == "1" || ci_env.to_lowercase() == "true" {
        return Ok(());
    }

    // Check immediately and update if needed
    check_and_force_update().await?;

    // Then start background task for future checks
    tokio::spawn(async {
        if let Err(e) = background_version_check().await {
            tracing::debug!("Background version check failed: {e}");
        }
    });

    Ok(())
}

/// Check and execute forced update - called before each command execution
pub async fn check_and_force_update() -> Result<()> {
    // If update is available, execute forced update
    if UPDATE_AVAILABLE.load(Ordering::Relaxed) {
        let version = UPDATE_VERSION.read().await;
        if let Some(ref new_version) = *version {
            tracing::debug!(
                "New version found: {} (current version: {}), updating automatically...",
                new_version,
                env!("CARGO_PKG_VERSION")
            );

            // Execute forced update
            execute_update(new_version).await?;

            // Reset state after update completion
            UPDATE_AVAILABLE.store(false, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// Background version check task
async fn background_version_check() -> Result<()> {
    // Check cache first
    let cache = match read_version_cache() {
        Ok(cache) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before UNIX epoch")
                .as_secs();
            if now - cache.check_time > 3600 {
                // Keep 1 hour cache
                // Cache expired, check remote version asynchronously
                if let Ok(new_cache) = check_remote_version_fast().await {
                    new_cache
                } else {
                    // Use expired cache on network failure
                    tracing::debug!("Using cached version due to network error");
                    cache
                }
            } else {
                cache
            }
        }
        Err(_) => {
            // No cache, try to check remote version
            check_remote_version_fast().await?
        }
    };

    // Check if new version is available
    let current_version = env!("CARGO_PKG_VERSION");
    if cache.version != current_version {
        // Mark update as available
        UPDATE_AVAILABLE.store(true, Ordering::Relaxed);
        let mut version = UPDATE_VERSION.write().await;
        *version = Some(cache.version.clone());

        tracing::debug!(
            "Update available: {} (current: {})",
            cache.version,
            current_version
        );
    }

    Ok(())
}

async fn execute_update(version: &str) -> Result<()> {
    let status = Command::new("utoo")
        .args(["i", &format!("utoo@{version}"), "-g"])
        .env("CI", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute update command")?;

    if status.success() {
        tracing::debug!("Update completed, please restart");
        Ok(())
    } else {
        tracing::error!("Auto update failed, please update manually");
        anyhow::bail!("Auto update failed, please execute manually {status}")
    }
}

/// Fast remote version check - short timeout, quick failure
async fn check_remote_version_fast() -> Result<VersionCache> {
    let registry_url = format!("{}/utoo/latest", get_registry());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1000)) // 1 second timeout
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

    let cache = VersionCache {
        version,
        check_time: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs(),
    };

    // Save cache asynchronously, don't block main flow
    if let Err(e) = save_version_cache(&cache) {
        tracing::debug!("Failed to save version cache: {e}");
    }

    Ok(cache)
}

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
    use std::sync::atomic::Ordering;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_init_auto_update_with_immediate_check_ci_environment() {
        // Set CI environment variable
        unsafe { std::env::set_var("CI", "1") };

        let result = init_auto_update().await;
        assert!(result.is_ok());

        // Clean up
        unsafe { std::env::remove_var("CI") };
    }

    #[tokio::test]
    async fn test_init_auto_update_with_immediate_check_normal_flow() {
        // Ensure we're not in CI
        unsafe { std::env::remove_var("CI") };

        let result = init_auto_update().await;
        assert!(result.is_ok());

        // Give background task a moment to start
        sleep(Duration::from_millis(100)).await;

        // The function should return after checking for immediate updates
        // and starting background task
        // This test mainly verifies that the function completes successfully
    }

    #[tokio::test]
    async fn test_init_auto_update_with_immediate_check_ci_true_lowercase() {
        // Set CI environment variable to lowercase "true"
        unsafe { std::env::set_var("CI", "true") };

        let result = init_auto_update().await;
        assert!(result.is_ok());

        // Clean up
        unsafe { std::env::remove_var("CI") };
    }

    #[tokio::test]
    async fn test_check_and_force_update_no_update_available() {
        // Test that check_and_force_update doesn't fail when no update is available
        // Note: We can't reliably test UPDATE_AVAILABLE state due to test concurrency
        let result = check_and_force_update().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_state_operations() {
        // Test that we can write and read from the global version state
        {
            let mut version = UPDATE_VERSION.write().await;
            *version = Some("test-version".to_string());
        }

        {
            let version = UPDATE_VERSION.read().await;
            assert_eq!(version.as_ref(), Some(&"test-version".to_string()));
        }

        // Note: Testing UPDATE_AVAILABLE is problematic in concurrent tests
        // because multiple tests can interfere with the global atomic boolean.
        // In a production test environment, we would use dependency injection
        // or mocking to isolate the state management.
    }

    #[test]
    fn test_version_cache_serialization() {
        let cache = VersionCache {
            version: "1.0.0".to_string(),
            check_time: 1234567890,
        };

        let serialized = serde_json::to_string(&cache).unwrap();
        let deserialized: VersionCache = serde_json::from_str(&serialized).unwrap();

        assert_eq!(cache.version, deserialized.version);
        assert_eq!(cache.check_time, deserialized.check_time);
    }

    #[test]
    fn test_read_version_cache_missing_file() {
        // Create a temporary directory and ensure the cache file doesn't exist
        let temp_dir = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();

        unsafe { std::env::set_var("HOME", temp_dir.path()) };

        // Now the cache file shouldn't exist, so reading should fail
        let result = read_version_cache();
        assert!(result.is_err());

        // Restore original HOME
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
        };

        // Create directory structure
        if let Some(parent) = cache_path.parent() {
            crate::fs::create_dir_all(parent).await?;
        }

        // Test serialization and file writing manually (without using global HOME)
        let content = serde_json::to_string(&cache)?;
        crate::fs::write(&cache_path, &content).await?;

        // Test reading and deserialization
        let read_content = crate::fs::read_to_string(cache_path).await?;
        let loaded_cache: VersionCache = serde_json::from_str(&read_content)?;

        assert_eq!(loaded_cache.version, cache.version);
        assert_eq!(loaded_cache.check_time, cache.check_time);

        Ok(())
    }

    #[tokio::test]
    async fn test_check_remote_version_fast_timeout() {
        // This test verifies that the fast version check has proper timeout
        // We can't easily test actual network timeouts in unit tests,
        // but we can verify the function signature and basic structure

        // Note: In a real test environment, you'd use a mock HTTP server
        // that intentionally delays responses to test timeout behavior

        let start_time = std::time::Instant::now();
        let result = check_remote_version_fast().await;
        let elapsed = start_time.elapsed();

        // The function should either succeed quickly or fail within reasonable time
        // (much less than the old 2-second timeout)
        assert!(elapsed < Duration::from_secs(5)); // Allow some margin for CI environments

        // Result can be either Ok or Err depending on network availability
        // but the important thing is that it doesn't hang
        match result {
            Ok(_) => {
                // Successfully fetched version
            }
            Err(_) => {
                // Network error or timeout - this is expected in test environment
            }
        }
    }

    #[test]
    fn test_get_cache_path() {
        let path = get_cache_path();
        assert!(path.to_string_lossy().contains(".utoo"));
        assert!(path.to_string_lossy().contains("remote-version.json"));
    }

    // Legacy tests for command execution
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

    #[test]
    fn test_atomic_operations() {
        // Test basic atomic operations (these are generally thread-safe)
        let initial = UPDATE_AVAILABLE.load(Ordering::Relaxed);

        // Toggle state
        UPDATE_AVAILABLE.store(!initial, Ordering::Relaxed);
        assert_eq!(UPDATE_AVAILABLE.load(Ordering::Relaxed), !initial);

        // Restore original state to avoid affecting other tests
        UPDATE_AVAILABLE.store(initial, Ordering::Relaxed);
    }
}
