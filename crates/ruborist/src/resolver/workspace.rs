//! Workspace discovery and resolution.
//!
//! This module provides platform-agnostic workspace discovery that works
//! across different environments (native CLI, WASM browser).
//!
//! # Architecture
//!
//! Workspace discovery requires:
//! 1. Reading package.json files
//! 2. Matching glob patterns (e.g., `packages/*`)
//! 3. Traversing directory structure
//!
//! Glob matching is delegated to the `FileSystem` trait, allowing each platform
//! to provide its own implementation (native uses `glob` crate, WASM can use OPFS).

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::model::package_json::PackageJson;
use crate::model::util::read_package_json;
use crate::service::FileSystem;

/// A discovered workspace package.
#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    /// Package name from package.json
    pub name: String,
    /// Path to the workspace directory
    pub path: PathBuf,
    /// The full package.json content
    pub package_json: PackageJson,
}

/// Workspace discovery context.
///
/// This struct holds the state needed for workspace discovery,
/// using a platform-agnostic FileSystem trait.
pub struct WorkspaceDiscovery<FS> {
    fs: FS,
}

impl<FS: FileSystem> WorkspaceDiscovery<FS> {
    /// Create a new workspace discovery context.
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Find all workspaces defined in the root package.json.
    ///
    /// This reads the package.json at `root_path`, extracts workspace patterns,
    /// and discovers all matching workspace packages.
    pub async fn find_workspaces(&self, root_path: &Path) -> Result<Vec<WorkspacePackage>> {
        let pkg = read_package_json(&self.fs, root_path).await?;
        self.find_workspaces_from_pkg(root_path, &pkg).await
    }

    /// Find workspaces from a pre-loaded package.json.
    pub async fn find_workspaces_from_pkg(
        &self,
        root_path: &Path,
        pkg: &PackageJson,
    ) -> Result<Vec<WorkspacePackage>> {
        let mut workspaces = Vec::new();

        let patterns = match &pkg.workspaces {
            Some(config) => config.patterns(),
            None => return Ok(workspaces),
        };

        for pattern_str in patterns {
            // Prepare glob pattern for package.json files
            let package_json_pattern = root_path.join(pattern_str).join("package.json");

            // Match workspace directories using FileSystem::glob
            let matched_paths = self
                .fs
                .glob(&package_json_pattern)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to glob pattern: {}", e))?;

            for path in matched_paths {
                let workspace_path = match path.parent() {
                    Some(p) => p.to_path_buf(),
                    None => continue,
                };

                // Read workspace package.json
                let workspace_pkg: PackageJson =
                    match read_package_json(&self.fs, &workspace_path).await {
                        Ok(pkg) => pkg,
                        Err(e) => {
                            tracing::debug!("Failed to read workspace package.json: {}", e);
                            continue;
                        }
                    };

                tracing::debug!(
                    "Found workspace: {} at {:?}",
                    workspace_pkg.name,
                    workspace_path
                );
                workspaces.push(WorkspacePackage {
                    name: workspace_pkg.name.clone(),
                    path: workspace_path,
                    package_json: workspace_pkg,
                });
            }
        }

        Ok(workspaces)
    }

    /// Check if a package.json indicates a workspace root.
    pub fn is_workspace_root(pkg: &PackageJson) -> bool {
        pkg.workspaces.is_some()
    }

    /// Find the project root by traversing up the directory tree.
    ///
    /// If the current directory is inside a workspace, returns the workspace root.
    /// Otherwise, returns the directory containing the closest package.json.
    pub async fn find_root_path(&self, cwd: &Path) -> Result<PathBuf> {
        // Find closest package.json
        let project_path = self.find_project_path(cwd).await?;

        let project_pkg = read_package_json(&self.fs, &project_path).await?;

        // If this is already a workspace root, return it
        if Self::is_workspace_root(&project_pkg) {
            return Ok(project_path);
        }

        // Look for parent workspace root
        if let Some(parent_root) = self.find_parent_workspace_root(&project_path).await? {
            // Check if current project is within a workspace pattern
            let parent_pkg = read_package_json(&self.fs, &parent_root).await?;

            if self
                .is_in_workspace(&project_path, &parent_root, &parent_pkg)
                .await?
            {
                tracing::debug!("Found workspace root at: {}", parent_root.display());
                return Ok(parent_root);
            }
        }

        Ok(project_path)
    }

    /// Find the closest directory containing package.json.
    pub async fn find_project_path(&self, cwd: &Path) -> Result<PathBuf> {
        // Check current directory first
        let current_pkg = cwd.join("package.json");
        if self
            .fs
            .exists(&current_pkg)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check file existence: {}", e))?
        {
            return Ok(cwd.to_path_buf());
        }

        // Traverse up
        let mut current = cwd.to_path_buf();
        while let Some(parent) = current.parent() {
            let pkg_path = parent.join("package.json");
            if self
                .fs
                .exists(&pkg_path)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to check file existence: {}", e))?
            {
                return Ok(parent.to_path_buf());
            }
            current = parent.to_path_buf();
        }

        // No package.json found, return original cwd
        Ok(cwd.to_path_buf())
    }

    /// Find parent directory that is a workspace root.
    async fn find_parent_workspace_root(&self, start: &Path) -> Result<Option<PathBuf>> {
        let mut current = start.to_path_buf();

        while let Some(parent) = current.parent() {
            let pkg_path = parent.join("package.json");
            if self
                .fs
                .exists(&pkg_path)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to check file existence: {}", e))?
            {
                let pkg = read_package_json(&self.fs, parent).await?;

                if Self::is_workspace_root(&pkg) {
                    return Ok(Some(parent.to_path_buf()));
                }
            }
            current = parent.to_path_buf();
        }

        Ok(None)
    }

    /// Check if a path is within a workspace pattern.
    async fn is_in_workspace(&self, path: &Path, root: &Path, pkg: &PackageJson) -> Result<bool> {
        let patterns = match &pkg.workspaces {
            Some(config) => config.patterns(),
            None => return Ok(false),
        };

        for pattern_str in patterns {
            let workspace_pattern = root.join(pattern_str);

            // Use FileSystem::glob for pattern matching
            let matched_paths = self
                .fs
                .glob(&workspace_pattern)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to glob pattern: {}", e))?;

            for entry in matched_paths {
                if path.starts_with(&entry) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }
}

/// Collect all package.json data from workspaces for dependency resolution.
///
/// This function reads all workspace package.json files and returns them
/// along with their paths for initializing the dependency graph.
pub async fn collect_workspace_packages<FS: FileSystem + Clone>(
    fs: FS,
    root_path: &Path,
    root_pkg: &PackageJson,
) -> Result<Vec<(PathBuf, PackageJson)>> {
    let discovery = WorkspaceDiscovery::new(fs);

    let workspaces = discovery
        .find_workspaces_from_pkg(root_path, root_pkg)
        .await?;

    Ok(workspaces
        .into_iter()
        .map(|ws| (ws.path, ws.package_json))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::package_json::WorkspacesConfig;
    use crate::service::NoopFileSystem;

    #[test]
    fn test_is_workspace_root() {
        let pkg_with_workspaces = PackageJson {
            name: "root".to_string(),
            workspaces: Some(WorkspacesConfig::Array(vec!["packages/*".to_string()])),
            ..Default::default()
        };
        assert!(WorkspaceDiscovery::<NoopFileSystem>::is_workspace_root(
            &pkg_with_workspaces
        ));

        let pkg_without_workspaces = PackageJson {
            name: "regular-pkg".to_string(),
            ..Default::default()
        };
        assert!(!WorkspaceDiscovery::<NoopFileSystem>::is_workspace_root(
            &pkg_without_workspaces
        ));
    }
}
