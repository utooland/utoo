use anyhow::{Context, Result};
use std::path::Path;

use crate::helper::lock::{
    serialize_tree_to_packages, write_ideal_tree_to_lock_file,
};
use crate::helper::ruborist::Ruborist;
use crate::service::workspace::WorkspaceService;
use crate::util::json::load_package_json_from_path;
use crate::util::logger::log_verbose;

/// Dependency resolution service
pub struct DependencyResolutionService;

impl DependencyResolutionService {
    pub async fn build_deps(cwd: &Path) -> Result<()> {
        let mut ruborist = Ruborist::new(cwd);
        ruborist.build_ideal_tree().await?;

        let pkg_file = load_package_json_from_path(cwd)?;

        const MAX_RETRIES: u32 = 5;
        let mut retry_count = 0;

        loop {
            let (pkgs_in_tree, _) = {
                let to_guard = ruborist.ideal_tree.as_ref().unwrap();
                serialize_tree_to_packages(to_guard, cwd)
            };

            let invalid_deps = Self::validate_deps(&pkg_file, &pkgs_in_tree).await?;

            if invalid_deps.is_empty() {
                log_verbose("No invalid dependencies found");
                break;
            }

            if retry_count >= MAX_RETRIES {
                return Err(anyhow::anyhow!(
                    "Failed to fix dependencies after {} retries",
                    MAX_RETRIES
                ));
            }

            for dep in invalid_deps {
                log_verbose(&format!(
                    "Fixing dependency: {}/{}",
                    dep.package_path, dep.dependency_name
                ));
                // Try to fix the dependency
                if let Err(e) = ruborist
                    .fix_dep_path(&dep.package_path, &dep.dependency_name)
                    .await
                {
                    log_verbose(&format!("Failed to fix dependency: {e}"));
                    return Err(anyhow::anyhow!("Failed to fix dependency: {}", e));
                } else {
                    log_verbose(&format!(
                        "Fixed dependency: {}/{}",
                        dep.package_path, dep.dependency_name
                    ));
                }
            }

            retry_count += 1;
        }

        let tree = ruborist.ideal_tree.unwrap();
        write_ideal_tree_to_lock_file(cwd, &tree).await?;

        Ok(())
    }

    pub async fn build_workspace(cwd: &Path) -> Result<()> {
        // Use the new workspace service to build the JSON
        let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;

        let temp_path = cwd.join("workspace.json.tmp");
        let target_path = cwd.join("workspace.json");

        std::fs::write(&temp_path, serde_json::to_string_pretty(&workspace_file)?)
            .context("Failed to write temporary workspace.json")?;
        std::fs::rename(temp_path, target_path).context("Failed to rename temporary workspace.json")?;

        Ok(())
    }

    pub async fn validate_deps(
        pkg_file: &serde_json::Value,
        pkgs_in_pkg_lock: &serde_json::Value,
    ) -> Result<Vec<crate::helper::lock::InvalidDependency>> {
        // Use the existing implementation from helper/lock.rs
        crate::helper::lock::validate_deps(pkg_file, pkgs_in_pkg_lock).await
    }
}