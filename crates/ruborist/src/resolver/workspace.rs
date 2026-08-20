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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::model::package_json::PackageJson;
use crate::model::util::read_package_json;
use crate::service::Glob;

/// Mirrors npm's `@npmcli/name-from-folder`: returns `<base>` for
/// `<parent>/<base>`, or `<parent>/<base>` when `<parent>` starts with `@`.
fn name_from_folder(path: &Path) -> Option<String> {
    let base = path.file_name()?.to_str()?;
    match path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
    {
        Some(parent) if parent.starts_with('@') => Some(format!("{parent}/{base}")),
        _ => Some(base.to_string()),
    }
}

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

fn ensure_unique_workspace_names(root_path: &Path, workspaces: &[WorkspacePackage]) -> Result<()> {
    let mut paths_by_name: BTreeMap<&str, Vec<&Path>> = BTreeMap::new();
    for workspace in workspaces {
        let paths = paths_by_name.entry(&workspace.name).or_default();
        if !paths.contains(&workspace.path.as_path()) {
            paths.push(&workspace.path);
        }
    }

    let mut duplicate_details = Vec::new();
    for (name, paths) in paths_by_name {
        if paths.len() < 2 {
            continue;
        }

        let mut paths = paths
            .into_iter()
            .map(|path| {
                path.strip_prefix(root_path)
                    .unwrap_or(path)
                    .join("package.json")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect::<Vec<_>>();
        paths.sort_unstable();
        let paths = paths
            .into_iter()
            .map(|path| format!("    {path}"))
            .collect::<Vec<_>>()
            .join("\n");
        duplicate_details.push(format!("  `{name}`:\n{paths}"));
    }

    if duplicate_details.is_empty() {
        return Ok(());
    }

    anyhow::bail!(
        "EDUPLICATEWORKSPACE: duplicate workspace package names detected:\n{}\n\
         Workspace package names must be unique. Rename the packages in their package.json files.",
        duplicate_details.join("\n")
    )
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

        for pattern_str in patterns {
            // Prepare glob pattern for package.json files
            let package_json_pattern = root_path.join(pattern_str).join("package.json");

            // Match workspace directories using Glob::glob
            let matched_paths = self
                .glob
                .glob(&package_json_pattern)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to glob pattern: {}", e))?;

            for path in matched_paths {
                let workspace_path = match path.parent() {
                    Some(p) => p.to_path_buf(),
                    None => continue,
                };

                // Read workspace package.json. A parse failure here drops the
                // member from the graph entirely, which later surfaces as a
                // confusing "workspace: dependency does not match any workspace
                // member" on any sibling that depends on it — so warn loudly
                // rather than swallowing it at debug level.
                let mut workspace_pkg: PackageJson = match read_package_json(&workspace_path).await
                {
                    Ok(pkg) => pkg,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping workspace member at {}: package.json failed to parse ({e:#}); \
                             `workspace:` deps on it will not resolve",
                            workspace_path.display()
                        );
                        continue;
                    }
                };

                // Anonymous workspaces (no `name` in package.json) get the
                // npm `@npmcli/name-from-folder` fallback written back into
                // the in-memory `PackageJson`, so every downstream consumer
                // sees a non-empty name without needing a parallel API.
                if workspace_pkg.name.is_empty() {
                    workspace_pkg.name = name_from_folder(&workspace_path).ok_or_else(|| {
                        anyhow::anyhow!(
                            "anonymous workspace at {} has no derivable name",
                            workspace_path.display()
                        )
                    })?;
                }
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

        ensure_unique_workspace_names(root_path, &workspaces)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::package_json::WorkspacesConfig;
    use crate::service::NoopGlob;

    fn workspace(name: &str, path: &str) -> WorkspacePackage {
        WorkspacePackage {
            name: name.to_string(),
            path: PathBuf::from(path),
            package_json: PackageJson {
                name: name.to_string(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn test_name_from_folder() {
        // Plain folder → its own basename.
        assert_eq!(
            name_from_folder(Path::new("/tmp/repo/packages/foo")),
            Some("foo".to_string())
        );
        // Folder under an `@scope` parent → "@scope/name" (matches npm).
        assert_eq!(
            name_from_folder(Path::new("/tmp/repo/packages/@scope/foo")),
            Some("@scope/foo".to_string())
        );
        // No parent at all (root path) → just the base.
        assert_eq!(name_from_folder(Path::new("foo")), Some("foo".to_string()));
    }

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

    #[test]
    fn test_duplicate_workspace_names_report_sorted_paths() {
        let workspaces = vec![
            workspace("shared", "/repo/packages/z"),
            workspace("other", "/repo/packages/other"),
            workspace("shared", "/repo/packages/a"),
        ];

        let error = ensure_unique_workspace_names(Path::new("/repo"), &workspaces)
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "EDUPLICATEWORKSPACE: duplicate workspace package names detected:\n  `shared`:\n    packages/a/package.json\n    packages/z/package.json\nWorkspace package names must be unique. Rename the packages in their package.json files."
        );
    }

    #[test]
    fn test_same_path_is_not_reported_as_duplicate() {
        let workspaces = vec![
            workspace("shared", "/repo/packages/a"),
            workspace("shared", "/repo/packages/a"),
        ];

        assert!(ensure_unique_workspace_names(Path::new("/repo"), &workspaces).is_ok());
    }
}
