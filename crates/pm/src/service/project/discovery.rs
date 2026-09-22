use super::context::Context as FsContext;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Find a workspace by name or path.
pub async fn find_workspace_path(cwd: &Path, workspace: &str) -> Result<PathBuf> {
    let workspaces = FsContext::discovery()
        .find_workspaces(cwd)
        .await
        .context("Failed to find workspaces")?;
    for ws in workspaces {
        // Try exact name match
        if ws.name == workspace {
            return Ok(ws.path);
        }

        // Try absolute path match
        if ws.path.to_string_lossy() == workspace {
            return Ok(ws.path);
        }

        // Try relative path match
        if let Ok(relative) = ws.path.strip_prefix(cwd)
            && relative.to_string_lossy() == workspace
        {
            return Ok(ws.path);
        }
    }
    anyhow::bail!("Workspace '{workspace}' not found")
}

/// Discover the workspace root without changing process state.
pub async fn project_root(cwd: &Path) -> Result<PathBuf> {
    FsContext::discovery().find_root_path(cwd).await
}

pub async fn package_directory(cwd: &Path) -> Result<PathBuf> {
    FsContext::discovery().find_project_path(cwd).await
}
