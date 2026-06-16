//! Workspace utilities for the CLI.
//!
//! This module provides CLI-specific workspace helpers that wrap
//! ruborist's platform-agnostic workspace discovery.

use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};

use super::ruborist_context::Context as FsContext;

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

/// Resolve the workspace root and change into it.
///
/// This is the standard entry point for commands that operate on the
/// project root (install, update, deps, etc.).
pub async fn init_project_root(cwd: &Path) -> Result<PathBuf> {
    let root_dir = FsContext::discovery().find_root_path(cwd).await?;
    chdir_if_needed(cwd, root_dir, "workspace root")
}

/// Update current working directory to project directory (closest package.json).
pub async fn update_cwd_to_project(cwd: &Path) -> Result<PathBuf> {
    let project_dir = FsContext::discovery().find_project_path(cwd).await?;
    chdir_if_needed(cwd, project_dir, "project")
}

/// `chdir` into `target` when it differs from `cwd`, logging with `label`;
/// returns `target` either way.
fn chdir_if_needed(cwd: &Path, target: PathBuf, label: &str) -> Result<PathBuf> {
    if !compare_paths(cwd, &target) {
        tracing::debug!("Changing directory to {label}: {}", target.display());
        env::set_current_dir(&target)
            .with_context(|| format!("Failed to change to {label} directory"))?;
    }
    Ok(target)
}

// Helper function to compare paths
fn compare_paths(left: &Path, right: &Path) -> bool {
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();
    let left = if let Some(stripped) = left.strip_prefix("/private") {
        stripped
    } else {
        &left
    };
    let right = if let Some(stripped) = right.strip_prefix("/private") {
        stripped
    } else {
        &right
    };
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    async fn setup_test_workspace() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path().to_path_buf();

        // Create root package.json with workspaces
        let root_pkg = r#"{
            "name": "root",
            "workspaces": ["packages/*"]
        }"#;
        fs::write(root_path.join("package.json"), root_pkg).unwrap();

        // Create workspace package.json
        let workspace_dir = root_path.join("packages").join("test-workspace");
        fs::create_dir_all(&workspace_dir).unwrap();
        let workspace_pkg = r#"{
            "name": "test-workspace"
        }"#;
        fs::write(workspace_dir.join("package.json"), workspace_pkg).unwrap();

        (temp_dir, root_path)
    }

    async fn setup_test_project() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_path_buf();

        // Create package.json without workspaces
        let pkg = r#"{
            "name": "test-project"
        }"#;
        fs::write(project_path.join("package.json"), pkg).unwrap();

        (temp_dir, project_path)
    }

    #[tokio::test]
    async fn test_find_project_path_in_workspace() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let workspace_path = root_path.join("packages").join("test-workspace");
        let found_project = FsContext::discovery()
            .find_project_path(&workspace_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_project, &workspace_path));
    }

    #[tokio::test]
    async fn test_find_project_path_in_root() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let found_project = FsContext::discovery()
            .find_project_path(&root_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_project, &root_path));
    }

    #[tokio::test]
    async fn test_find_project_path_in_project() {
        let (_temp_dir, project_path) = setup_test_project().await;
        let found_project = FsContext::discovery()
            .find_project_path(&project_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_project, &project_path));
    }

    #[tokio::test]
    async fn test_find_project_path_no_package_json() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().to_path_buf();
        let found_project = FsContext::discovery()
            .find_project_path(&test_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_project, &test_path));
    }

    #[tokio::test]
    async fn test_init_project_root_in_root() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        init_project_root(&root_path).await.unwrap();
        let result = update_cwd_to_project(&root_path).await.unwrap();
        assert!(compare_paths(&result, &root_path));
    }

    #[tokio::test]
    async fn test_update_cwd_to_project_in_workspace() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let workspace_path = root_path.join("packages").join("test-workspace");

        // Test that update_cwd_to_project correctly handles workspace path
        let result = update_cwd_to_project(&workspace_path).await.unwrap();
        assert!(compare_paths(&result, &workspace_path));
    }

    #[tokio::test]
    async fn test_update_cwd_to_project_in_root() {
        let (_temp_dir, root_path) = setup_test_workspace().await;

        // Test that update_cwd_to_project correctly handles root path
        let result = update_cwd_to_project(&root_path).await.unwrap();
        assert!(compare_paths(&result, &root_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_workspace_root() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let found_root = FsContext::discovery()
            .find_root_path(&root_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_root, &root_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_workspace_package() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let workspace_path = root_path.join("packages").join("test-workspace");
        let found_root = FsContext::discovery()
            .find_root_path(&workspace_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_root, &root_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_workspace_subdir() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let subdir_path = root_path
            .join("packages")
            .join("test-workspace")
            .join("src");
        fs::create_dir_all(&subdir_path).unwrap();
        let found_root = FsContext::discovery()
            .find_root_path(&subdir_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_root, &root_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_independent_project() {
        let (_temp_dir, project_path) = setup_test_project().await;
        let found_root = FsContext::discovery()
            .find_root_path(&project_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_root, &project_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_project_subdir() {
        let (_temp_dir, project_path) = setup_test_project().await;
        let subdir_path = project_path.join("src");
        fs::create_dir_all(&subdir_path).unwrap();
        let found_root = FsContext::discovery()
            .find_root_path(&subdir_path)
            .await
            .unwrap();
        assert!(compare_paths(&found_root, &project_path));
    }
}
