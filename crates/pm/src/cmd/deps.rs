use anyhow::{Context as _, Result};
use std::path::Path;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::service::build_deps as ruborist_build_deps;

use crate::helper::fs::Context;
use crate::helper::lock::save_package_lock;
use crate::service::pipeline::PipelineInstaller;
use crate::service::workspace::WorkspaceService;
use crate::util::logger::{finish_progress_bar, start_progress_bar};

pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
    start_progress_bar();

    let (options, channels) = Context::build_deps_options(cwd.to_path_buf()).await;

    // Start pipeline workers for concurrent tgz downloading
    let installer = PipelineInstaller::new();
    let handles = installer.start_workers(channels, cwd.to_path_buf());

    let package_lock = ruborist_build_deps(options).await?;

    // Wait for all downloads to complete
    handles.await_completion().await;

    finish_progress_bar("package-lock.json resolved");

    // Save to disk
    save_package_lock(cwd, &package_lock).await?;

    Ok(package_lock)
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;
    let content = serde_json::to_string_pretty(&workspace_file)?;
    crate::fs::write(cwd.join("workspace.json"), content)
        .await
        .context("Failed to write workspace.json")
}
