use anyhow::{Context, Result};
use std::path::Path;

use crate::helper::lock::{PackageLock, build_ideal_tree_to_package_lock};
use crate::helper::ruborist::Ruborist;
use crate::service::workspace::WorkspaceService;

/// Dependency resolution service
pub struct DependencyResolutionService;

impl DependencyResolutionService {
    pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
        let mut ruborist = Ruborist::new(cwd);
        ruborist.build_ideal_tree().await?;

        let graph = ruborist.ideal_tree.as_ref().unwrap();

        // Serialize graph to packages
        let (_packages, _total) = graph.serialize_to_packages(cwd);

        // Build package lock from graph
        let package_lock = build_ideal_tree_to_package_lock(cwd, graph).await?;

        Ok(package_lock)
    }

    pub async fn build_workspace(cwd: &Path) -> Result<()> {
        // Use the new workspace service to build the JSON
        let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;

        let temp_path = cwd.join("workspace.json.tmp");
        let target_path = cwd.join("workspace.json");

        tokio::fs::write(&temp_path, serde_json::to_string_pretty(&workspace_file)?)
            .await
            .context("Failed to write temporary workspace.json")?;
        tokio::fs::rename(temp_path, target_path)
            .await
            .context("Failed to rename temporary workspace.json")?;

        Ok(())
    }
}
