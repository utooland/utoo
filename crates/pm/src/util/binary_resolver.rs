use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// Find a binary in node_modules/.bin directories, searching up the directory tree
pub async fn find_binary(command: &str) -> Result<Option<PathBuf>> {
    let current_dir = std::env::current_dir()?;
    find_binary_in_hierarchy(&current_dir, command).await
}

/// Find a binary in the utoo cache directory
/// Logic:
/// Return the first executable file found in the bin directory
pub fn find_binary_in_cache(package_cache_dir: &Path) -> Result<Option<PathBuf>> {
    let bin_dir = package_cache_dir.join("bin");

    if !bin_dir.exists() {
        return Ok(None);
    }

    // Get the first file in bin directory
    let entries = fs::read_dir(&bin_dir)?;

    for entry in entries {
        let entry = entry?;
        let path: PathBuf = entry.path();
        if path.is_file() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// Search for binary in the directory hierarchy starting from the given path
async fn find_binary_in_hierarchy(start_path: &Path, command: &str) -> Result<Option<PathBuf>> {
    let mut current_path = start_path.to_path_buf();

    loop {
        let bin_path = current_path.join("node_modules").join(".bin").join(command);

        // Check if the binary exists and is executable
        if bin_path.exists() {
            return Ok(Some(bin_path));
        }

        // Move up to parent directory
        if let Some(parent) = current_path.parent() {
            current_path = parent.to_path_buf();
        } else {
            // Reached the root directory
            break;
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs as async_fs;

    // Helper function to create a test directory structure
    async fn create_test_structure() -> Result<TempDir> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();

        // Create node_modules/.bin directory
        let bin_dir = base_path.join("node_modules").join(".bin");
        async_fs::create_dir_all(&bin_dir).await?;

        // Create a test binary file
        let test_binary = bin_dir.join("test-cmd");
        async_fs::write(&test_binary, "#!/bin/bash\necho 'test'").await?;

        // Set executable permissions on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = async_fs::metadata(&test_binary).await?.permissions();
            perms.set_mode(0o755);
            async_fs::set_permissions(&test_binary, perms).await?;
        }

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn test_find_binary_in_hierarchy_found() {
        let temp_dir = create_test_structure().await.unwrap();
        let start_path = temp_dir.path();

        let result = find_binary_in_hierarchy(start_path, "test-cmd")
            .await
            .unwrap();
        assert!(result.is_some());

        let found_path = result.unwrap();
        assert!(found_path.ends_with("node_modules/.bin/test-cmd"));
        assert!(found_path.exists());
    }

    #[tokio::test]
    async fn test_find_binary_in_hierarchy_not_found() {
        let temp_dir = create_test_structure().await.unwrap();
        let start_path = temp_dir.path();

        let result = find_binary_in_hierarchy(start_path, "nonexistent-cmd")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_find_binary_in_hierarchy_search_up() {
        let temp_dir = create_test_structure().await.unwrap();
        let base_path = temp_dir.path();

        // Create a subdirectory without node_modules
        let sub_dir = base_path.join("subdir").join("deeper");
        async_fs::create_dir_all(&sub_dir).await.unwrap();

        // Search from the subdirectory should find the binary in parent
        let result = find_binary_in_hierarchy(&sub_dir, "test-cmd")
            .await
            .unwrap();
        assert!(result.is_some());

        let found_path = result.unwrap();
        assert!(found_path.ends_with("node_modules/.bin/test-cmd"));
    }
}
