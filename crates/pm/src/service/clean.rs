use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use anyhow::{Context, Result};
use glob::glob;
use utoo_ruborist::util::PackageNameStr;

use crate::helper::lock::Package;
use crate::helper::ruborist_context::Context as FsContext;

/// Remove a symlink with platform-specific handling.
///
/// On Windows, directory symlinks require `remove_dir` instead of `remove_file`.
async fn remove_symlink(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(windows)]
    {
        let metadata = crate::fs::symlink_metadata(path).await?;
        if !metadata.file_type().is_symlink() {
            return Ok(());
        }

        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        if (metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY) != 0 {
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

/// Check if a directory name matches legacy npminstall pattern (e.g. `_pkg@1.0.0@2.0.0`).
fn is_legacy_npminstall(name: &str) -> bool {
    let at_count = name.matches('@').count();
    name.starts_with('_') && (at_count == 2 || at_count == 4)
}

/// Remove symlinks inside a scoped package directory (e.g. `node_modules/@scope/`).
async fn remove_scoped_symlinks(scope_dir: &Path) -> Result<()> {
    let Ok(mut entries) = crate::fs::read_dir(scope_dir).await else {
        return Ok(());
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if crate::fs::symlink_metadata(&path)
            .await?
            .file_type()
            .is_symlink()
        {
            tracing::debug!("Removing scoped symlink: {}", path.display());
            remove_symlink(&path).await.ok();
        }
    }
    Ok(())
}

/// Remove stale symlinks and legacy npminstall artifacts from a node_modules directory.
async fn remove_stale_entries(node_modules: &Path) -> Result<()> {
    let Ok(mut entries) = crate::fs::read_dir(node_modules).await else {
        return Ok(());
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let metadata = crate::fs::symlink_metadata(&path).await?;

        if metadata.file_type().is_symlink() {
            tracing::debug!("Removing symlink: {}", path.display());
            remove_symlink(&path).await.ok();
            continue;
        }

        if !metadata.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.is_scoped() {
            remove_scoped_symlinks(&path).await?;
        } else if is_legacy_npminstall(&name) {
            tracing::debug!("Removing legacy package: {}", path.display());
            crate::fs::remove_dir_all(&path).await.ok();
        }
    }

    Ok(())
}

/// Remove packages not in `valid_packages`, recursing into nested node_modules.
async fn remove_unused_packages(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &HashSet<String>,
) -> Result<()> {
    let mut stack = vec![node_modules.to_path_buf()];

    while let Some(current_nm) = stack.pop() {
        let patterns = [
            current_nm.join("*/package.json"),
            current_nm.join("@*/*/package.json"),
        ];
        for pattern in &patterns {
            let pattern_str = pattern.to_string_lossy();
            let entries = glob(&pattern_str)
                .with_context(|| format!("Glob failed for pattern: {pattern_str}"))?;

            for entry in entries {
                let pkg_json = entry.with_context(|| format!("Glob entry error: {pattern_str}"))?;
                let pkg_dir = pkg_json
                    .parent()
                    .context("Failed to get parent of package.json")?;
                let rel_path = pkg_dir.strip_prefix(cwd).with_context(|| {
                    format!(
                        "Failed to strip prefix {} from {}",
                        cwd.display(),
                        pkg_dir.display()
                    )
                })?;

                if let Some(pkg_name) = Package::path_to_pkg_name(&pkg_dir.to_string_lossy())
                    && !valid_packages.contains(rel_path.to_string_lossy().as_ref())
                {
                    tracing::debug!("Cleaning unused package: {pkg_name}");
                    crate::fs::remove_dir_all(pkg_dir).await.ok();
                }

                let nested_nm = pkg_dir.join("node_modules");
                if crate::fs::try_exists(&nested_nm).await? {
                    stack.push(nested_nm);
                }
            }
        }
    }

    Ok(())
}

/// Clean unused dependencies across all workspace node_modules directories.
pub async fn clean_deps(groups: &HashMap<usize, Vec<(String, Package)>>, cwd: &Path) -> Result<()> {
    let valid_packages: HashSet<String> = groups
        .values()
        .flat_map(|pkgs| pkgs.iter().map(|(path, _)| path.clone()))
        .collect();

    tracing::debug!("Valid packages: {valid_packages:?}");

    let mut nm_dirs = vec![cwd.join("node_modules")];
    for ws in FsContext::discovery().find_workspaces(cwd).await? {
        let ws_nm = ws.path.join("node_modules");
        if crate::fs::try_exists(&ws_nm).await? {
            tracing::debug!("Adding workspace node_modules: {}", ws_nm.display());
            nm_dirs.push(ws_nm);
        }
    }

    for nm_dir in &nm_dirs {
        remove_stale_entries(nm_dir).await?;
        remove_unused_packages(nm_dir, cwd, &valid_packages).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_remove_symlink() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let target = temp_dir.path().join("utoo-cli");
        let link = temp_dir.path().join("symlink");

        fs::create_dir(&target).await?;

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link)?;

        assert!(fs::symlink_metadata(&link).await?.file_type().is_symlink());

        remove_symlink(&link).await?;

        assert!(!link.exists());
        assert!(target.exists());
        Ok(())
    }

    #[tokio::test]
    async fn test_remove_scoped_symlinks() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let scope_dir = temp_dir.path().join("@utoo");
        fs::create_dir(&scope_dir).await?;

        let target = temp_dir.path().join("utoo-cli");
        let link = scope_dir.join("cli");
        fs::create_dir(&target).await?;

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link)?;

        assert!(fs::symlink_metadata(&link).await?.file_type().is_symlink());

        remove_scoped_symlinks(&scope_dir).await?;

        assert!(!link.exists());
        assert!(target.exists());
        Ok(())
    }

    #[tokio::test]
    async fn test_is_legacy_npminstall() {
        assert!(is_legacy_npminstall("_utoo-cli@1.0.0@2.0.0"));
        assert!(!is_legacy_npminstall("lodash"));
        assert!(!is_legacy_npminstall("_lodash@1.0.0"));
    }
}
