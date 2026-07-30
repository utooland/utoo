use std::path::Path;
use std::time::Instant;

use anyhow::{Context as _, Result};
use utoo_ruborist::lock::PackageLock;

use crate::helper::lock::save_package_lock;
use crate::helper::ruborist_context::Context;
use crate::helper::workspace::init_project_root;
use crate::model::cli_output::{DependenciesSummary, DepsResult, WorkspaceSummary};
use crate::service::workspace::WorkspaceService;
use crate::util::logger::{finish_progress_bar, log_time_end, start_progress_bar};
use crate::util::presenter::emit;

/// Entry point for the `deps` command.
pub async fn run(workspace_only: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root_path = init_project_root(&cwd).await?;
    let output = if workspace_only {
        let workspace = build_workspace(&root_path).await?;
        DepsResult::Workspace {
            output_path: root_path
                .join("workspace.json")
                .to_string_lossy()
                .into_owned(),
            summary: WorkspaceSummary {
                workspaces: workspace.node_list.len() as u64,
                edges: workspace.edges.len() as u64,
                layers: workspace.topology.len() as u64,
            },
        }
    } else {
        let lock = build_deps(&root_path).await?;
        DepsResult::Dependencies {
            output_path: root_path
                .join("package-lock.json")
                .to_string_lossy()
                .into_owned(),
            summary: DependenciesSummary {
                packages: lock.packages.len().saturating_sub(1) as u64,
            },
        }
    };
    log_time_end("deps resolved");
    emit("deps", &output, || Ok(()))
}

pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
    start_progress_bar();
    let resolve_start = Instant::now();

    let lock = Context::build_deps(cwd.to_path_buf()).await?;

    finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));

    // Save to disk
    save_package_lock(cwd, &lock).await?;

    Ok(lock)
}

pub async fn build_workspace(cwd: &Path) -> Result<crate::service::workspace::WorkspaceJson> {
    let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;
    let content = serde_json::to_string_pretty(&workspace_file)?;
    crate::fs::write(cwd.join("workspace.json"), content)
        .await
        .context("Failed to write workspace.json")?;
    Ok(workspace_file)
}
