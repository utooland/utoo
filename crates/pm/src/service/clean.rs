use anyhow::{Context, Result};
use glob::glob;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::helper::lock::{Package, path_to_pkg_name};
use crate::helper::workspace;

/// Check if a path is a symlink using async metadata
async fn is_symlink_async(path: &Path) -> Result<bool> {
    Ok(crate::fs::symlink_metadata(path)
        .await?
        .file_type()
        .is_symlink())
}

/// Remove a symlink with proper platform-specific handling
async fn remove_symlink_cross_platform(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        // On Windows, we need to check if the symlink points to a directory
        let metadata = crate::fs::symlink_metadata(path).await?;

        if !metadata.file_type().is_symlink() {
            return Ok(());
        }

        // Check if it's a directory symlink by checking the file attributes
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        let is_dir_symlink = (metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY) != 0;

        if is_dir_symlink {
            crate::fs::remove_dir(path).await
        } else {
            crate::fs::remove_file(path).await
        }
    }

    #[cfg(not(windows))]
    {
        crate::fs::remove_file(path).await
    }
}

/// Clean up a single node_modules directory
async fn clean_node_modules_dir(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &HashSet<String>,
) -> Result<()> {
    // clean up symlinks for npminstall
    if let Ok(mut entries) = crate::fs::read_dir(node_modules).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if is_symlink_async(&path).await? {
                clean_symlink(&path).await?;
            } else if path.is_dir() {
                clean_directory(&path).await?;
            }
        }
    }

    clean_unused_packages(node_modules, cwd, valid_packages).await?;

    Ok(())
}

/// Clean up a symlink
async fn clean_symlink(path: &Path) -> Result<()> {
    tracing::debug!("Removing symlink: {}", path.display());

    if let Err(e) = remove_symlink_cross_platform(path).await {
        tracing::debug!("Failed to remove symlink {}: {}", path.display(), e);
    }

    Ok(())
}

/// Clean up a directory, handling scoped packages and legacy npm install packages
async fn clean_directory(path: &Path) -> Result<()> {
    let Some(file_name) = path.file_name() else {
        return Ok(());
    };
    let name = file_name.to_string_lossy();
    if name.starts_with('@') {
        clean_scoped_package(path).await
    } else {
        clean_legacy_npminstall_package(path, &name).await
    }
}

/// Clean up a scoped package directory
async fn clean_scoped_package(path: &Path) -> Result<()> {
    if let Ok(mut scope_entries) = crate::fs::read_dir(path).await {
        while let Ok(Some(scope_entry)) = scope_entries.next_entry().await {
            let scope_path = scope_entry.path();

            // Use async metadata check for symlink
            if is_symlink_async(&scope_path).await? {
                tracing::debug!("Removing scoped symlink: {}", scope_path.display());

                if let Err(e) = remove_symlink_cross_platform(&scope_path).await {
                    tracing::debug!(
                        "Failed to remove scoped symlink {}: {}",
                        scope_path.display(),
                        e
                    );
                }
            }
        }
    }
    Ok(())
}

/// Clean up a legacy npminstall package
async fn clean_legacy_npminstall_package(path: &Path, name: &str) -> Result<()> {
    let at_count = name.matches('@').count();
    if name.starts_with('_') && (at_count == 2 || at_count == 4) {
        tracing::debug!("Removing legacy package: {}", path.display());
        if let Err(e) = crate::fs::remove_dir_all(path).await {
            tracing::debug!("Failed to remove legacy package {}: {}", path.display(), e);
        }
    }
    Ok(())
}

/// Clean up unused packages in the node_modules directory
async fn clean_unused_packages(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &HashSet<String>,
) -> Result<()> {
    // Helper function for recursive search
    fn find_and_clean<'a>(
        node_modules: &'a Path,
        cwd: &'a Path,
        valid_packages: &'a HashSet<String>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let patterns = [
                node_modules.join("*/package.json"),
                node_modules.join("@*/*/package.json"),
            ];
            for pattern in patterns.iter() {
                let pattern_str = pattern.to_string_lossy().to_string();
                for entry in glob(&pattern_str)
                    .with_context(|| format!("Glob failed for pattern: {pattern_str}"))?
                {
                    let pkg_json_path = entry
                        .with_context(|| format!("Glob entry error for pattern: {pattern_str}"))?;
                    let pkg_dir = pkg_json_path
                        .parent()
                        .context("Failed to get parent directory of package.json")?;
                    if let Some(pkg_name) = path_to_pkg_name(&pkg_dir.to_string_lossy()) {
                        let pkg_path = pkg_dir.strip_prefix(cwd).with_context(|| {
                            format!(
                                "Failed to strip prefix {} from {}",
                                cwd.display(),
                                pkg_dir.display()
                            )
                        })?;
                        if !valid_packages.contains(pkg_path.to_string_lossy().as_ref()) {
                            tracing::debug!("Cleaning unused package: {pkg_name}");
                            if let Err(e) = crate::fs::remove_dir_all(pkg_dir).await {
                                tracing::debug!("Failed to remove {pkg_name}: {e}");
                            }
                        }
                    }
                    // Recursively check nested node_modules
                    let nested_node_modules = pkg_dir.join("node_modules");
                    if crate::fs::try_exists(&nested_node_modules).await? {
                        find_and_clean(&nested_node_modules, cwd, valid_packages).await?;
                    }
                }
            }
            Ok(())
        })
    }
    find_and_clean(node_modules, cwd, valid_packages).await?;
    Ok(())
}

/// Clean unused dependencies across all workspace node_modules directories
pub async fn clean_deps(groups: &HashMap<usize, Vec<(String, Package)>>, cwd: &Path) -> Result<()> {
    let mut valid_packages = HashSet::new();
    for packages in groups.values() {
        for (path, _) in packages {
            valid_packages.insert(path.clone());
        }
    }

    tracing::debug!("Valid packages: {valid_packages:?}");

    let mut node_modules_dirs = vec![cwd.join("node_modules")];

    let workspaces = workspace::find_workspaces(cwd).await?;
    for (_, path, _) in workspaces {
        let workspace_node_modules = path.join("node_modules");
        if crate::fs::try_exists(&workspace_node_modules).await? {
            tracing::debug!(
                "add workspace node_modules: {:?}",
                workspace_node_modules.display()
            );
            node_modules_dirs.push(workspace_node_modules);
        }
    }

    // cleanup unused packages in all workspace_members
    for node_modules in node_modules_dirs {
        clean_node_modules_dir(&node_modules, cwd, &valid_packages).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_clean_symlink() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let target_dir = temp_dir.path().join("utoo-cli");
        let symlink_path = temp_dir.path().join("symlink");

        // Create target directory
        fs::create_dir(&target_dir).await?;

        // Create symlink
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlink_path)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target_dir, &symlink_path)?;

        // Verify symlink exists
        assert!(is_symlink_async(&symlink_path).await?);

        // Test cleaning
        clean_symlink(&symlink_path).await?;

        // Verify symlink is removed
        assert!(!symlink_path.exists());
        assert!(target_dir.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_clean_scoped_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let scope_dir = temp_dir.path().join("@utoo");
        fs::create_dir(&scope_dir).await?;

        // Create a symlink in the scope directory
        let target_dir = temp_dir.path().join("utoo-cli");
        let symlink_path = scope_dir.join("cli");
        fs::create_dir(&target_dir).await?;

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlink_path)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target_dir, &symlink_path)?;

        // Verify symlink exists
        assert!(is_symlink_async(&symlink_path).await?);

        // Test cleaning
        clean_scoped_package(&scope_dir).await?;

        // Verify symlink is removed
        assert!(!symlink_path.exists());
        assert!(target_dir.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_clean_legacy_npminstall_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let legacy_dir = temp_dir.path().join("_utoo-cli@1.0.0@2.0.0");
        fs::create_dir(&legacy_dir).await?;

        // Test cleaning
        clean_legacy_npminstall_package(&legacy_dir, "_utoo-cli@1.0.0@2.0.0").await?;

        // Verify directory is removed
        assert!(!legacy_dir.exists());

        Ok(())
    }
}
