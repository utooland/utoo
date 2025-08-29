use anyhow::{Context, Result};
use glob::glob;
use serde_json::Value;
use std::env;
use std::path::{Path, PathBuf};

use crate::util::{
    json::{load_package_json_from_path, read_json_file},
    logger::{log_info, log_verbose},
};

pub async fn find_workspaces(root_path: &Path) -> Result<Vec<(String, PathBuf, Value)>> {
    let mut workspaces = Vec::new();
    let pkg = load_package_json_from_path(root_path)?;

    // load workspaces config
    if let Some(workspaces_config) = pkg.get("workspaces") {
        match workspaces_config {
            Value::Array(patterns) => {
                for pattern in patterns {
                    if let Some(pattern_str) = pattern.as_str() {
                        // prepare glob pattern
                        let package_json_path = root_path.join(pattern_str).join("package.json");
                        let glob_pattern = package_json_path.to_str().ok_or_else(|| {
                            anyhow::anyhow!(
                                "Invalid path encoding: {}",
                                package_json_path.display()
                            )
                        })?;

                        // glob match
                        for entry in glob(glob_pattern)
                            .context(format!("Invalid glob pattern: {glob_pattern}"))?
                        {
                            match entry {
                                Ok(path) => {
                                    // load package.json in workspace
                                    let workspace_pkg =
                                        read_json_file::<Value>(&path).context(format!(
                                            "Failed to parse workspace package.json at {}",
                                            path.display()
                                        ))?;

                                    // load workspace name
                                    let name =
                                        workspace_pkg["name"].as_str().unwrap_or("").to_string();

                                    // get workspace path
                                    let workspace_path = path
                                        .parent()
                                        .ok_or_else(|| {
                                            anyhow::anyhow!(
                                                "Invalid workspace path: {}",
                                                path.display()
                                            )
                                        })?
                                        .to_path_buf();

                                    log_verbose(&format!("Found workspace: {name} {path:?}"));
                                    workspaces.push((name, workspace_path, workspace_pkg));
                                }
                                Err(e) => log_verbose(&format!("Error processing workspace: {e}")),
                            }
                        }
                    }
                }
            }
            _ => log_verbose("Workspaces field is not an array"),
        }
    }

    Ok(workspaces)
}

pub async fn find_workspace_path(cwd: &Path, workspace: &str) -> Result<PathBuf> {
    let workspaces = find_workspaces(cwd)
        .await
        .context("Failed to find workspaces")?;
    for (name, path, _) in workspaces {
        // Try exact name match
        if name == workspace {
            return Ok(path);
        }

        // Try absolute path match
        if path.to_string_lossy() == workspace {
            return Ok(path);
        }

        // Try relative path match
        if let Ok(relative) = path.strip_prefix(cwd)
            && relative.to_string_lossy() == workspace
        {
            return Ok(path);
        }
    }
    anyhow::bail!("Workspace '{}' not found", workspace)
}

/// Check if a directory is a workspace root by examining its package.json
async fn is_workspace_root(pkg: &Value) -> bool {
    pkg.get("workspaces").is_some()
}

/// Check if a directory is within a workspace pattern
async fn is_in_workspace(cwd: &Path, root: &Path, pattern: &str) -> Result<bool> {
    let workspace_path = root.join(pattern);
    let glob_pattern = workspace_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid path encoding: {}", workspace_path.display()))?;

    if let Ok(entries) = glob(glob_pattern) {
        for path in entries.flatten() {
            if cwd.starts_with(&path) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Find the closest directory containing package.json by traversing up
async fn find_closest_parent_pkg(start_dir: &Path) -> Result<Option<(PathBuf, Value)>> {
    let mut current = start_dir.to_path_buf();

    while let Some(parent) = current.parent() {
        let package_json_path = parent.join("package.json");
        if package_json_path.exists() {
            let pkg = read_json_file::<Value>(&package_json_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to read package.json at {}: {}",
                    package_json_path.display(),
                    e
                )
            })?;
            return Ok(Some((parent.to_path_buf(), pkg)));
        }
        current = parent.to_path_buf();
    }

    Ok(None)
}

/// Find the project root path by traversing up the directory tree
/// root
///   -a
///     -package.json
///   -b
///     -package.json
///     -c
///       -d
/// will return root path
pub async fn find_root_path(cwd: &Path) -> Result<PathBuf> {
    // Find closet package.json location
    let project_path = find_project_path(cwd).await?;
    let project_pkg = load_package_json_from_path(&project_path)?;

    // is workspace root, return project path directly
    if is_workspace_root(&project_pkg).await {
        return Ok(project_path);
    }

    let (parent_project_dir, parent_pkg) = match find_closest_parent_pkg(&project_path).await? {
        Some((dir, pkg)) => (dir, pkg),
        None => return Ok(project_path),
    };

    // parent is not workspace root, return project path directly
    if !is_workspace_root(&parent_pkg).await {
        return Ok(project_path);
    }
    let patterns = match parent_pkg.get("workspaces") {
        Some(Value::Array(patterns)) => patterns,
        _ => return Ok(parent_project_dir),
    };
    for pattern in patterns {
        let pattern_str = match pattern.as_str() {
            Some(s) => s,
            None => continue,
        };
        if is_in_workspace(&project_path, &parent_project_dir, pattern_str).await? {
            log_verbose(&format!(
                "Found workspace root at: {}",
                parent_project_dir.display()
            ));
            return Ok(parent_project_dir);
        }
    }
    Ok(project_path)
}

/// Update current working directory to project root (with workspaces)
pub async fn update_cwd_to_root(cwd: &Path) -> Result<PathBuf> {
    let root_dir = find_root_path(cwd).await?;
    if !compare_paths(cwd, &root_dir) {
        log_info(&format!(
            "Changing directory to workspace root: {}",
            root_dir.display()
        ));
        env::set_current_dir(&root_dir).context("Failed to change to root directory")?;
    }
    Ok(root_dir)
}

/// Update current working directory to project directory (closest package.json)
pub async fn update_cwd_to_project(cwd: &Path) -> Result<PathBuf> {
    let project_dir = find_project_path(cwd).await?;
    if !compare_paths(cwd, &project_dir) {
        log_info(&format!(
            "Changing directory to project: {}",
            project_dir.display()
        ));
        env::set_current_dir(&project_dir).context("Failed to change to project directory")?;
    }
    Ok(project_dir)
}

/// Find the closest directory containing package.json by traversing up
pub async fn find_project_path(cwd: &Path) -> Result<PathBuf> {
    // First check if current directory has package.json
    let current_package_json = cwd.join("package.json");
    if current_package_json.exists() {
        return Ok(cwd.to_path_buf());
    }

    // If not, traverse up
    let (pkg_dir, _) = match find_closest_parent_pkg(cwd).await? {
        Some((dir, pkg)) => (dir, pkg),
        None => return Ok(cwd.to_path_buf()),
    };

    Ok(pkg_dir)
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
        let found_project = find_project_path(&workspace_path).await.unwrap();
        assert!(compare_paths(&found_project, &workspace_path));
    }

    #[tokio::test]
    async fn test_find_project_path_in_root() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let found_project = find_project_path(&root_path).await.unwrap();
        assert!(compare_paths(&found_project, &root_path));
    }

    #[tokio::test]
    async fn test_find_project_path_in_project() {
        let (_temp_dir, project_path) = setup_test_project().await;
        let found_project = find_project_path(&project_path).await.unwrap();
        assert!(compare_paths(&found_project, &project_path));
    }

    #[tokio::test]
    async fn test_find_project_path_no_package_json() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().to_path_buf();
        let found_project = find_project_path(&test_path).await.unwrap();
        assert!(compare_paths(&found_project, &test_path));
    }

    #[tokio::test]
    async fn test_update_cwd_to_root_in_root() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        update_cwd_to_root(&root_path).await.unwrap();
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
        let found_root = find_root_path(&root_path).await.unwrap();
        assert!(compare_paths(&found_root, &root_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_workspace_package() {
        let (_temp_dir, root_path) = setup_test_workspace().await;
        let workspace_path = root_path.join("packages").join("test-workspace");
        let found_root = find_root_path(&workspace_path).await.unwrap();
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
        let found_root = find_root_path(&subdir_path).await.unwrap();
        assert!(compare_paths(&found_root, &root_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_independent_project() {
        let (_temp_dir, project_path) = setup_test_project().await;
        let found_root = find_root_path(&project_path).await.unwrap();
        assert!(compare_paths(&found_root, &project_path));
    }

    #[tokio::test]
    async fn test_find_root_path_in_project_subdir() {
        let (_temp_dir, project_path) = setup_test_project().await;
        let subdir_path = project_path.join("src");
        fs::create_dir_all(&subdir_path).unwrap();
        let found_root = find_root_path(&subdir_path).await.unwrap();
        assert!(compare_paths(&found_root, &project_path));
    }
}
