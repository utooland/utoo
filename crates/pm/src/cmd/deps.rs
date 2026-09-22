use std::path::Path;

use anyhow::{Context as _, Result};

use crate::cmd::project::init_project_root;
use crate::model::cli_output::{DependenciesSummary, DepsResult, WorkspaceSummary};
use crate::service::project::resolve_and_save_lock;
use crate::service::workspace::WorkspaceService;
use crate::util::logger::ProgressReceiver;
use crate::util::logger::log_time_end;
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
        let lock = resolve_and_save_lock(&root_path, ProgressReceiver).await?;
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

pub async fn build_workspace(cwd: &Path) -> Result<crate::service::workspace::WorkspaceJson> {
    let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;
    let content = serde_json::to_string_pretty(&workspace_file)?;
    crate::fs::write(cwd.join("workspace.json"), content)
        .await
        .context("Failed to write workspace.json")?;
    Ok(workspace_file)
}
