//! CLI process handoff after asynchronous release preparation.
use crate::helper::self_pin::{HANDOFF_ENV, PreparedHandoff};
use anyhow::{Context, Result};

pub(crate) async fn handoff(prepared: PreparedHandoff, args: &[String]) -> Result<()> {
    if !crate::util::invocation::quiet() {
        eprintln!(
            "utoo: using pinned utoo@{} from {}",
            prepared.version,
            prepared.executable.display()
        );
    }
    utoo_ruborist::util::task::wait_for_idle().await;
    crate::util::manifest_store::finish_pending_writers().await;
    handoff_prepared(prepared, args).await
}

#[cfg(unix)]
async fn handoff_prepared(prepared: PreparedHandoff, args: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let PreparedHandoff {
        executable,
        version,
        lock: _lock,
    } = prepared;
    // The lock stays open until exec closes CLOEXEC descriptors.
    let error = std::process::Command::new(&executable)
        .args(args)
        .env(HANDOFF_ENV, version)
        .env_remove("UTOO_MANAGED_PACKAGE_ROOT")
        .exec();
    Err(error).with_context(|| format!("Failed to start pinned Utoo at {}", executable.display()))
}

#[cfg(windows)]
async fn handoff_prepared(prepared: PreparedHandoff, args: &[String]) -> Result<()> {
    let PreparedHandoff {
        executable,
        version,
        lock,
    } = prepared;
    let child = tokio::process::Command::new(&executable)
        .args(args)
        .env(HANDOFF_ENV, version)
        .env_remove("UTOO_MANAGED_PACKAGE_ROOT")
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to start pinned Utoo at {}", executable.display()))?;
    // CreateProcess has opened the executable. Release before waiting so the
    // pinned child can clean its cache without deadlocking against the parent.
    drop(lock);
    let status = crate::service::script::ScriptService::wait_inherited(child)
        .await
        .with_context(|| format!("Failed to wait for pinned Utoo at {}", executable.display()))?;
    Err(super::CommandExit(status.code().unwrap_or(1)).into())
}
