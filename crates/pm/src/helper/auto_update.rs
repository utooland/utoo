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
use crate::util::invocation;
use crate::util::user_config::get_registry;
use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

#[derive(Serialize, Deserialize, Debug)]
struct VersionCache {
    version: String,
    check_time: u64,
    /// Timestamp of last failed update attempt; skip retry until cooldown expires.
    #[serde(default)]
    last_update_failed: Option<u64>,
}

pub(crate) const INTERNAL_UPDATE_ENV: &str = "UTOO_INTERNAL_UPDATE";

const CACHE_TTL_SECS: u64 = 3600; // 1 hour
const UPDATE_RETRY_COOLDOWN_SECS: u64 = 86400; // 24 hours

/// The CLI owns the check and awaits any installation already started.
#[derive(Default)]
pub(crate) struct AutoUpdate {
    cancel: Option<oneshot::Sender<()>>,
    control: Arc<UpdateControl>,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
struct UpdateControl(Mutex<bool>);
impl UpdateControl {
    fn spawn_if_active<T>(&self, spawn: impl FnOnce() -> Result<T>) -> Result<Option<T>> {
        let cancelled = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *cancelled {
            return Ok(None);
        }
        // Serialize cancellation with the actual process spawn. Once spawn
        // returns, the task owns a child and must wait even if the CLI exits.
        spawn().map(Some)
    }
}

impl AutoUpdate {
    fn start<C, F, U>(check: C, install: F) -> Self
    where
        C: std::future::Future<Output = Option<String>> + Send + 'static,
        F: FnOnce(String, Arc<UpdateControl>) -> U + Send + 'static,
        U: std::future::Future<Output = ()> + Send + 'static,
    {
        let (cancel, cancelled) = oneshot::channel();
        let control = Arc::new(UpdateControl::default());
        let worker_control = control.clone();
        let task = utoo_ruborist::util::task::spawn(async move {
            let version = tokio::select! {
                biased;
                _ = cancelled => return,
                version = check => version,
            };
            if let Some(version) = version {
                install(version, worker_control).await;
            }
        });
        Self {
            cancel: Some(cancel),
            control,
            task: Some(task),
        }
    }

    async fn wait(mut self) {
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
        {
            tracing::debug!("Auto update task failed: {error}");
        }
    }

    pub(crate) async fn finish(mut self) {
        *self
            .control
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.cancel.take();
        self.wait().await;
    }
}
impl Drop for AutoUpdate {
    fn drop(&mut self) {
        *self
            .control
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.cancel.take();
    }
}

/// Preserve synchronous force/fresh-cache updates; return ownership of the
/// background cache refresh so command completion can cancel only its check.
pub async fn init_auto_update() -> AutoUpdate {
    if std::env::var_os(INTERNAL_UPDATE_ENV).is_some()
        || invocation::quiet()
        || crate::helper::self_pin::is_active()
    {
        return AutoUpdate::default();
    }
    let force = std::env::var("UTOO_FORCE_UPDATE").is_ok_and(|v| v == "1" || v == "true");
    let ci = std::env::var("CI").unwrap_or_default();
    if !force && (APP_VERSION == "0.0.0" || ci == "1" || ci.eq_ignore_ascii_case("true")) {
        return AutoUpdate::default();
    }
    let cache_path = get_cache_path();
    let cached = (!force)
        .then(|| read_version_cache(&cache_path))
        .and_then(Result::ok)
        .filter(|cache| !is_cache_expired(cache));
    let synchronous = force || cached.is_some();
    let check_path = cache_path.clone();
    let registry = get_registry();
    let update = AutoUpdate::start(
        async move {
            let version = if let Some(cache) = cached {
                cache.version
            } else {
                match fetch_and_cache_version(&registry, &check_path).await {
                    Ok(version) => version,
                    Err(error) => {
                        tracing::debug!("Version fetch failed: {error}");
                        return None;
                    }
                }
            };
            (version != APP_VERSION).then_some(version)
        },
        move |version, control| async move {
            try_update(&version, &cache_path, &control).await;
        },
    );
    if synchronous {
        update.wait().await;
        AutoUpdate::default()
    } else {
        update
    }
}

/// Attempt silent update; print a prompt on failure.
/// Respects a 24h cooldown after a failed attempt to avoid nagging.
async fn try_update(new_version: &str, cache_path: &Path, control: &UpdateControl) {
    // Check cooldown from last failed attempt
    if let Ok(cache) = read_version_cache(cache_path)
        && let Some(failed_at) = cache.last_update_failed
    {
        let now = now_secs();
        if now.saturating_sub(failed_at) < UPDATE_RETRY_COOLDOWN_SECS {
            tracing::debug!(
                "Skipping update (last failure {}s ago, cooldown {}s)",
                now.saturating_sub(failed_at),
                UPDATE_RETRY_COOLDOWN_SECS,
            );
            return;
        }
    }

    match execute_update(new_version, control).await {
        Ok(false) => {}
        Ok(true) => {
            println!("{}", "Updated successfully.".green());
            mark_update_failed(cache_path, None);
        }
        Err(e) => {
            tracing::debug!("Auto update failed: {e}");
            mark_update_failed(cache_path, Some(now_secs()));
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
    now_secs().saturating_sub(cache.check_time) > CACHE_TTL_SECS
}

/// Write (or clear) the `last_update_failed` timestamp in the cache file.
fn mark_update_failed(cache_path: &Path, timestamp: Option<u64>) {
    if let Ok(mut cache) = read_version_cache(cache_path) {
        cache.last_update_failed = timestamp;
        if let Err(e) = save_version_cache(cache_path, &cache) {
            tracing::debug!("Failed to save version cache: {e}");
        }
    }
}

async fn execute_update(version: &str, control: &UpdateControl) -> Result<bool> {
    let Some(child) = control.spawn_if_active(|| {
        println!(
            "{} utoo: {} → {}",
            "Updating".cyan(),
            APP_VERSION.yellow(),
            version.green(),
        );
        update_command(version)?
            .spawn()
            .context("Failed to execute update command")
    })?
    else {
        return Ok(false);
    };
    let status = crate::service::script::ScriptService::wait_inherited(child).await?;
    anyhow::ensure!(status.success(), "update command exited with {status}");
    Ok(true)
}

fn update_command(version: &str) -> Result<tokio::process::Command> {
    let executable = std::env::current_exe().context("Failed to locate current utoo executable")?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(["i", &format!("utoo@{version}"), "-g"])
        .env(INTERNAL_UPDATE_ENV, "1")
        .env("CI", "1")
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

/// Fetch latest version from registry, write to cache file, and return the version.
async fn fetch_and_cache_version(registry: &str, cache_path: &Path) -> Result<String> {
    let registry_url = format!("{registry}/utoo/latest");
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

    let last_update_failed = read_version_cache(cache_path)
        .ok()
        .and_then(|c| c.last_update_failed);
    let cache = VersionCache {
        version,
        check_time: now_secs(),
        last_update_failed,
    };

    save_version_cache(cache_path, &cache)?;
    Ok(cache.version)
}

// ── Cache I/O ──────────────────────────────────────────────

fn get_cache_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".utoo").join("remote-version.json")
}

fn read_version_cache(cache_path: &Path) -> Result<VersionCache> {
    let content = fs::read_to_string(cache_path).context("Failed to read version cache file")?;
    serde_json::from_str(&content).context("Failed to parse version cache")
}

fn save_version_cache(cache_path: &Path, cache: &VersionCache) -> Result<()> {
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

    #[tokio::test]
    async fn command_completion_cancels_a_check_before_installation() {
        let (started, ready) = oneshot::channel();
        let (release, blocked) = oneshot::channel();
        let update = AutoUpdate::start(
            async move {
                started.send(()).unwrap();
                blocked.await.unwrap();
                Some("version".into())
            },
            |_, _| async { panic!("cancelled check must not install") },
        );
        ready.await.unwrap();
        update.finish().await;
        assert!(release.send(()).is_err());
    }

    #[tokio::test]
    async fn completion_between_check_and_spawn_prevents_installation() {
        let (ready, started) = oneshot::channel();
        let (release, blocked) = oneshot::channel();
        let update = AutoUpdate::start(
            async { Some("version".into()) },
            move |_, control| async move {
                ready.send(()).unwrap();
                blocked.await.unwrap();
                assert!(
                    control
                        .spawn_if_active(|| -> Result<()> { panic!("late installation") })
                        .unwrap()
                        .is_none()
                );
            },
        );
        started.await.unwrap();
        let finish = update.finish();
        tokio::pin!(finish);
        assert!(futures::poll!(&mut finish).is_pending());
        release.send(()).unwrap();
        finish.await;
    }

    #[tokio::test]
    async fn completion_waits_for_an_already_started_installer() {
        let (started, ready) = oneshot::channel();
        let (finished, completed) = oneshot::channel();
        let update = AutoUpdate::start(
            async { Some("version".into()) },
            move |_, control| async move {
                let mut child = control.spawn_if_active(|| {
                Ok(tokio::process::Command::new("node").args(["-e", "process.stdin.resume();process.stdin.on('end',()=>process.exit(0))"])
                    .stdin(Stdio::piped()).kill_on_drop(true).spawn()?)
            }).unwrap().unwrap();
                started.send(child.stdin.take().unwrap()).unwrap();
                let status = crate::service::script::ScriptService::wait_inherited(child)
                    .await
                    .unwrap();
                finished.send(status.success()).unwrap();
            },
        );
        let input = ready.await.unwrap();
        let finish = update.finish();
        tokio::pin!(finish);
        assert!(futures::poll!(&mut finish).is_pending());
        drop(input);
        finish.await;
        assert!(
            completed.await.unwrap(),
            "installer was killed instead of awaited"
        );
    }

    #[test]
    fn update_uses_current_executable_and_marks_internal_child() {
        let command = update_command("1.2.3").unwrap();
        let command = command.as_std();
        assert_eq!(
            command.get_program(),
            std::env::current_exe().unwrap().as_os_str()
        );
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == INTERNAL_UPDATE_ENV
                    && value == Some(std::ffi::OsStr::new("1")))
        );
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
        let temp = tempfile::tempdir().unwrap();
        assert!(read_version_cache(&temp.path().join("missing.json")).is_err());
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
        let temp = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let registry = format!("http://{}", listener.local_addr().unwrap());
        let result = fetch_and_cache_version(&registry, &temp.path().join("version.json")).await;
        let elapsed = start_time.elapsed();

        assert!(elapsed < Duration::from_secs(5));
        assert!(result.is_err());
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
