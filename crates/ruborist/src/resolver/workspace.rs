//! Workspace discovery and resolution.
//!
//! This module provides platform-agnostic workspace discovery that works
//! across different environments (native CLI, WASM browser).
//!
//! # Architecture
//!
//! Workspace discovery requires:
//! 1. Reading package.json files (via tokio-fs-ext)
//! 2. Matching glob patterns (e.g., `packages/*`) via Glob trait
//! 3. Traversing directory structure
//!
//! Glob matching is delegated to the `Glob` trait, allowing each platform
//! to provide its own implementation (native uses `glob` crate, WASM can use OPFS).

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::model::package_json::PackageJson;
use crate::model::util::read_package_json;
use crate::service::Glob;

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
/// using platform-agnostic Glob trait for pattern matching.
pub struct WorkspaceDiscovery<G> {
    glob: G,
}

impl<G: Glob> WorkspaceDiscovery<G> {
    /// Create a new workspace discovery context.
    pub fn new(glob: G) -> Self {
        Self { glob }
    }

    /// Find all workspaces defined in the root package.json.
    ///
    /// This reads the package.json at `root_path`, extracts workspace patterns,
    /// and discovers all matching workspace packages.
    pub async fn find_workspaces(&self, root_path: &Path) -> Result<Vec<WorkspacePackage>> {
        let pkg = read_package_json(root_path).await?;
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

        // Collect every matched workspace directory across all patterns
        // first, then read each `package.json` concurrently. The previous
        // serial `for path in matched_paths { read_package_json(...).await }`
        // paid one FS round-trip per workspace on the resolver hot path —
        // ant-design has ~200 workspaces and the read takes the largest
        // unmeasured chunk of `p1_resolve` between glob completion and
        // BFS start.
        let mut workspace_paths: Vec<PathBuf> = Vec::new();
        for pattern_str in patterns {
            let package_json_pattern = root_path.join(pattern_str).join("package.json");
            let matched_paths = self
                .glob
                .glob(&package_json_pattern)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to glob pattern: {}", e))?;
            for path in matched_paths {
                if let Some(p) = path.parent() {
                    workspace_paths.push(p.to_path_buf());
                }
            }
        }

        // Fan out the per-workspace package.json reads. Each is small
        // (typical workspace package.json < 4 KB) so completion order
        // doesn't matter — push results into `workspaces` as they land.
        use futures::stream::{FuturesUnordered, StreamExt};
        let mut futs: FuturesUnordered<_> = workspace_paths
            .into_iter()
            .map(|workspace_path| async move {
                let pkg = read_package_json(&workspace_path).await;
                (workspace_path, pkg)
            })
            .collect();

        while let Some((workspace_path, pkg)) = futs.next().await {
            match pkg {
                Ok(workspace_pkg) => {
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
                Err(e) => {
                    tracing::debug!("Failed to read workspace package.json: {}", e);
                }
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

        let project_pkg = read_package_json(&project_path).await?;

        // If this is already a workspace root, return it
        if Self::is_workspace_root(&project_pkg) {
            return Ok(project_path);
        }

        // Look for parent workspace root
        if let Some(parent_root) = self.find_parent_workspace_root(&project_path).await? {
            // Check if current project is within a workspace pattern
            let parent_pkg = read_package_json(&parent_root).await?;

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
        if crate::service::exists(&current_pkg).await {
            return Ok(cwd.to_path_buf());
        }

        // Traverse up
        let mut current = cwd.to_path_buf();
        while let Some(parent) = current.parent() {
            let pkg_path = parent.join("package.json");
            if crate::service::exists(&pkg_path).await {
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
            if crate::service::exists(&pkg_path).await {
                let pkg = read_package_json(parent).await?;

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

            // Use Glob::glob for pattern matching
            let matched_paths = self
                .glob
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
pub async fn collect_workspace_packages<G: Glob + Clone>(
    glob: G,
    root_path: &Path,
    root_pkg: &PackageJson,
) -> Result<Vec<(PathBuf, PackageJson)>> {
    let discovery = WorkspaceDiscovery::new(glob);

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
    use crate::service::NoopGlob;

    #[test]
    fn test_is_workspace_root() {
        let pkg_with_workspaces = PackageJson {
            name: "root".to_string(),
            workspaces: Some(WorkspacesConfig::Array(vec!["packages/*".to_string()])),
            ..Default::default()
        };
        assert!(WorkspaceDiscovery::<NoopGlob>::is_workspace_root(
            &pkg_with_workspaces
        ));

        let pkg_without_workspaces = PackageJson {
            name: "regular-pkg".to_string(),
            ..Default::default()
        };
        assert!(!WorkspaceDiscovery::<NoopGlob>::is_workspace_root(
            &pkg_without_workspaces
        ));
    }
}
