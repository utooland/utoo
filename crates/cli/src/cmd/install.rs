use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::sync::Semaphore;

use crate::cmd::rebuild::rebuild;
use crate::helper::lock::update_package_json;
use crate::helper::lock::{
    PackageLock, ensure_package_lock, group_by_depth, prepare_global_package_json,
};
use crate::helper::workspace::update_cwd_to_root;
use crate::model::package::PackageInfo;
use crate::service::install::install_packages;
use crate::util::cache::get_cache_dir;
use crate::util::logger::finish_progress_bar;
use crate::util::logger::log_verbose;
use crate::util::logger::start_progress_bar;
use crate::util::logger::{PROGRESS_BAR, log_info};
use crate::util::save_type::PackageAction;
use crate::util::save_type::SaveType;

use super::deps::build_deps;

pub async fn update_package(
    action: PackageAction,
    spec: &str,
    workspace: Option<String>,
    ignore_scripts: bool,
    save_type: SaveType,
) -> Result<()> {
    log_verbose(&format!(
        "update package: {:?} {:?} {:?} {:?}",
        action, spec, &workspace, ignore_scripts
    ));
    let cwd = std::env::current_dir().context("Failed to get current directory")?;

    // 1. Update package.json and package-lock.json
    update_package_json(&action, spec, &workspace, &save_type)
        .await
        .context("Failed to update package.json")?;

    // 2. Update working directory to project root (if in workspace)
    let root_path = update_cwd_to_root(&cwd).await?;

    // 3. Rebuild Deps
    build_deps(&root_path)
        .await
        .context("Failed to build package-lock.json")?;

    install(ignore_scripts, &root_path)
        .await
        .context("Failed to install packages")?;

    Ok(())
}

pub async fn install(ignore_scripts: bool, root_path: &Path) -> Result<()> {
    // Package lock prerequisite check
    ensure_package_lock(root_path).await?;

    // load package-lock.json
    let package_lock: PackageLock = serde_json::from_reader(
        fs::File::open(root_path.join("package-lock.json"))
            .context("Failed to open package-lock.json")?,
    )
    .map_err(|e| anyhow::anyhow!("Failed to parse package-lock.json: {}", e))?;

    let cache_dir = get_cache_dir();

    let groups = group_by_depth(&package_lock.packages);

    let mut depths: Vec<_> = groups.keys().cloned().collect();
    depths.sort_unstable();
    start_progress_bar();
    PROGRESS_BAR.set_length(package_lock.packages.len() as u64);

    // Get the number of logical CPU cores of the system and set it to twice the number of CPU cores
    let concurrent_limit = thread::available_parallelism()
        .map(|n| n.get() * 2)
        .unwrap_or(20)
        .max(20);
    log_verbose(&format!("Setting concurrent limit to {concurrent_limit}"));
    let semaphore = Arc::new(Semaphore::new(concurrent_limit));

    install_packages(&groups, &cache_dir, root_path, semaphore)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to install packages: {}", e))?;

    finish_progress_bar("node_modules cloned finished");

    if !ignore_scripts {
        log_info(
            "Starting to execute dependency hook scripts, you can add --ignore-scripts to skip",
        );
        rebuild(root_path).await?;
        log_info("💫 All dependencies installed successfully");
        Ok(())
    } else {
        log_info(
            "💫 All dependencies installed successfully (you can run 'utoo rebuild' to trigger dependency hooks)",
        );
        Ok(())
    }
}

pub async fn install_global_package(npm_spec: &str, prefix: &Option<String>) -> Result<()> {
    // Prepare global package directory and package.json
    let package_path = prepare_global_package_json(npm_spec, prefix)
        .await
        .context("Failed to prepare global package.json")?;

    log_verbose(&format!("Installing global package: {npm_spec}"));

    // Install dependencies
    install(false, &package_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to install global package dependencies: {}", e))?;

    // Create package info from path
    let package_info =
        PackageInfo::from_path(&package_path).context("Failed to create package info from path")?;

    let target_bin_dir = match prefix {
        Some(prefix) => PathBuf::from(prefix).join("bin"),

        // If prefix is not set, link to the bin directory of the current executable
        // ~/.nvm/versions/node/v20.15.0/bin/utoo -> ~/.nvm/versions/node/v20.15.0/bin
        None => std::env::current_exe()
            .context("Failed to get current executable path")?
            .parent()
            .context("Failed to get executable parent directory")?
            .to_path_buf(),
    };

    // Link binary files to global
    log_verbose(&format!(
        "Linking binary files to global... {}",
        target_bin_dir.display()
    ));
    package_info
        .link_to_global(&target_bin_dir)
        .await
        .context("Failed to link binary files to global")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_install_global_package_invalid_spec() {
        // Test installing with invalid package spec
        let result = install_global_package("invalid-package-that-does-not-exist", &None).await;
        assert!(result.is_err(), "Should fail with invalid package spec");
    }
}
