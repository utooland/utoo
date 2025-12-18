use anyhow::{Context as _, Result};
use std::path::Path;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::service::build_deps as ruborist_build_deps;

use crate::helper::fs::Context;
use crate::helper::lock::save_package_lock;
use crate::service::workspace::WorkspaceService;
use crate::util::logger::{finish_progress_bar, start_progress_bar};

pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
    start_progress_bar();

    let options = Context::build_deps_options(cwd.to_path_buf()).await;
    let package_lock = ruborist_build_deps(options).await?;

    finish_progress_bar("package-lock.json resolved");

    // Save to disk
    save_package_lock(cwd, &package_lock).await?;

    Ok(package_lock)
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;
    let content = serde_json::to_string_pretty(&workspace_file)?;
    tokio::fs::write(cwd.join("workspace.json"), content)
        .await
        .context("Failed to write workspace.json")
}
