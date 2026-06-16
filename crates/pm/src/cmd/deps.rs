use std::path::Path;
use std::time::Instant;

use anyhow::{Context as _, Result};
use utoo_ruborist::lock::PackageLock;

use crate::helper::lock::save_package_lock;
use crate::helper::ruborist_context::Context;
use crate::helper::workspace::init_project_root;
use crate::service::workspace::WorkspaceService;
use crate::util::logger::{finish_progress_bar, log_time_end, start_progress_bar};

/// Entry point for the `deps` command.
pub async fn run(workspace_only: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root_path = init_project_root(&cwd).await?;
    if workspace_only {
        build_workspace(&root_path).await?;
    } else {
        // Ignore returned PackageLock for CLI command
        build_deps(&root_path).await?;
    }
    log_time_end("deps resolved");
    Ok(())
}

pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
    start_progress_bar();
    let resolve_start = Instant::now();

    let output = Context::build_deps(cwd.to_path_buf()).await?;

    finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));

    // Save to disk
    save_package_lock(cwd, &output.lock).await?;

    Ok(output.lock)
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;
    let content = serde_json::to_string_pretty(&workspace_file)?;
    crate::fs::write(cwd.join("workspace.json"), content)
        .await
        .context("Failed to write workspace.json")
}
