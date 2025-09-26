use anyhow::Context;
use anyhow::Result;
use glob::glob;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::cmd::rebuild::rebuild;
use crate::helper::global_bin::get_global_bin_dir;
use crate::helper::lock::{
    Package, PackageLock, ensure_package_lock, extract_package_name, group_by_depth,
    path_to_pkg_name, prepare_global_package_json, update_package_json,
};
use crate::helper::workspace;
use crate::helper::{is_cpu_compatible, is_os_compatible};
use crate::model::package::PackageInfo;
use crate::util::cache::get_cache_dir;
use crate::util::cloner::clone;
use crate::util::downloader::download;
use crate::util::linker::link;
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_info, log_progress, log_verbose, start_progress_bar,
};
use crate::util::save_type::{PackageAction, SaveType};

use super::binary::update_package_binary;

static CONCURRENT_LIMIT: usize = 40;

/// Clean up a single node_modules directory
async fn clean_node_modules_dir(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &std::collections::HashSet<String>,
) -> Result<()> {
    // clean up symlinks for npminstall
    if let Ok(mut entries) = tokio::fs::read_dir(node_modules).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_symlink() {
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
    log_verbose(&format!("Removing symlink: {}", path.display()));
    if let Err(e) = tokio::fs::remove_file(path).await {
        log_verbose(&format!(
            "Failed to remove symlink {}: {}",
            path.display(),
            e
        ));
    }
    Ok(())
}

/// Clean up a directory, handling scoped packages and legacy npm install packages
async fn clean_directory(path: &Path) -> Result<()> {
    if let Some(file_name) = path.file_name()
        && let Some(name) = file_name.to_str()
    {
        if name.starts_with('@') {
            clean_scoped_package(path).await?;
        } else {
            clean_legacy_npminstall_package(path, name).await?;
        }
    }
    Ok(())
}

/// Clean up a scoped package directory
async fn clean_scoped_package(path: &Path) -> Result<()> {
    if let Ok(mut scope_entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(scope_entry)) = scope_entries.next_entry().await {
            let scope_path = scope_entry.path();
            if scope_path.is_symlink() {
                log_verbose(&format!(
                    "Removing scoped symlink: {}",
                    scope_path.display()
                ));
                if let Err(e) = tokio::fs::remove_file(&scope_path).await {
                    log_verbose(&format!(
                        "Failed to remove scoped symlink {}: {}",
                        scope_path.display(),
                        e
                    ));
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
        log_verbose(&format!("Removing legacy package: {}", path.display()));
        if let Err(e) = tokio::fs::remove_dir_all(path).await {
            log_verbose(&format!(
                "Failed to remove legacy package {}: {}",
                path.display(),
                e
            ));
        }
    }
    Ok(())
}

/// Clean up unused packages in the node_modules directory
async fn clean_unused_packages(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &std::collections::HashSet<String>,
) -> Result<()> {
    // Helper function for recursive search
    fn find_and_clean<'a>(
        node_modules: &'a Path,
        cwd: &'a Path,
        valid_packages: &'a std::collections::HashSet<String>,
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
                            log_verbose(&format!("Cleaning unused package: {pkg_name}"));
                            if let Err(e) = tokio::fs::remove_dir_all(pkg_dir).await {
                                log_verbose(&format!("Failed to remove {pkg_name}: {e}"));
                            }
                        }
                    }
                    // Recursively check nested node_modules
                    let nested_node_modules = pkg_dir.join("node_modules");
                    if tokio::fs::try_exists(&nested_node_modules).await? {
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

async fn clean_deps(groups: &HashMap<usize, Vec<(String, Package)>>, cwd: &Path) -> Result<()> {
    let mut valid_packages = std::collections::HashSet::new();
    for packages in groups.values() {
        for (path, _) in packages {
            valid_packages.insert(path.clone());
        }
    }

    log_verbose(&format!("Valid packages: {valid_packages:?}"));

    let mut node_modules_dirs = vec![cwd.join("node_modules")];

    let workspaces = workspace::find_workspaces(cwd).await?;
    for (_, path, _) in workspaces {
        let workspace_node_modules = path.join("node_modules");
        if tokio::fs::try_exists(&workspace_node_modules).await? {
            node_modules_dirs.push(workspace_node_modules.clone());
            log_verbose(&format!(
                "add workspace node_modules: {:?}",
                workspace_node_modules.display()
            ));
        }
    }

    // cleanup unused packages in all workspace_members
    for node_modules in node_modules_dirs {
        clean_node_modules_dir(&node_modules, cwd, &valid_packages).await?;
    }

    Ok(())
}

pub async fn install_packages(
    groups: &HashMap<usize, Vec<(std::string::String, Package)>>,
    cache_dir: &Path,
    cwd: &Path,
    semaphore: Arc<Semaphore>,
) -> Result<()> {
    // clean unused deps
    clean_deps(groups, cwd).await?;

    let mut depths: Vec<_> = groups.keys().cloned().collect();
    depths.sort_unstable();

    for depth in depths.iter() {
        if let Some(packages) = groups.get(depth) {
            let mut tasks: Vec<tokio::task::JoinHandle<Result<()>>> = Vec::new();
            for (path, package) in packages.iter() {
                let path = path.clone();
                let package = package.clone();
                if let Some(resolved) = package.resolved {
                    if package.link.is_some() {
                        let link_name = extract_package_name(&path);
                        if link_name.is_empty() {
                            PROGRESS_BAR.inc(1);
                            log_verbose(&format!("Link skipped due to empty package name: {path}"));
                            continue;
                        }
                        log_verbose(&format!("Attempting to link from {resolved} to {path}"));
                        if let Err(e) = link(Path::new(&resolved), Path::new(&path)).await {
                            log_verbose(&format!(
                                "Link failed: source={resolved}, target={path}, error={e}"
                            ));
                            return Err(anyhow::anyhow!("Link failed: {e}"));
                        }
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    // skip when cpu or os is not compatible
                    if package.cpu.is_some() && !is_cpu_compatible(&package.cpu.unwrap()) {
                        PROGRESS_BAR.inc(1);
                        log_verbose(&format!("cpu skipped: {}", &path));
                        continue;
                    }

                    if package.os.is_some() && !is_os_compatible(&package.os.unwrap()) {
                        PROGRESS_BAR.inc(1);
                        log_verbose(&format!("os skipped: {}", &path));
                        continue;
                    }

                    let name = package.name.unwrap_or_else(|| extract_package_name(&path));
                    let version = package.version.as_ref().unwrap();
                    let cache_path = cache_dir.join(format!("{name}/{version}"));
                    let cache_flag_path = cache_dir.join(format!("{name}/{version}/_resolved"));
                    let cwd_clone = cwd.to_path_buf();
                    let should_resolve = !tokio::fs::try_exists(&cache_flag_path).await?;
                    let semaphore = Arc::clone(&semaphore);

                    let task = tokio::spawn(async move {
                        let _permit = semaphore.acquire().await.unwrap();
                        if should_resolve {
                            log_verbose(&format!("Downloading {path} to {name}"));
                            match download(&resolved, &cache_path).await {
                                Ok(_) => {
                                    log_progress(&format!("{name} downloaded"));
                                    if package.has_install_script.is_some() {
                                        log_verbose(&format!("{name} has install script"));
                                        let has_install_script_flag_path =
                                            cache_path.join("_hasInstallScript");
                                        tokio::fs::write(has_install_script_flag_path, "").await?;
                                    }
                                }
                                Err(e) => {
                                    log_verbose(&format!(
                                        "Download failed: source={}, target={}, error={}",
                                        resolved,
                                        cache_path.display(),
                                        e
                                    ));
                                    return Err(anyhow::anyhow!("{name} download failed: {e}"));
                                }
                            }
                        }

                        log_verbose(&format!("{name} clone"));
                        match clone(&cache_path, &cwd_clone.join(&path), true).await {
                            Ok(_) => {
                                log_verbose(&format!("{name} resolved"));
                                PROGRESS_BAR.inc(1);
                                log_progress(&format!("{name} resolved"));
                                update_package_binary(&cwd_clone.join(&path), &name).await?;
                                Ok(())
                            }
                            Err(e) => Err(anyhow::anyhow!(
                                "Copy failed {} to {}: {}",
                                cache_path.display(),
                                cwd_clone.join(&path).display(),
                                e
                            )),
                        }
                    });
                    tasks.push(task);
                } else {
                    PROGRESS_BAR.inc(1);
                    log_verbose(&format!("{path} no resolved info skipped"));
                }
            }

            for task in tasks {
                match task.await {
                    Ok(Ok(())) => continue,
                    Ok(Err(e)) => {
                        log_verbose(&format!("Task execution error: {e}"));
                        return Err(anyhow::anyhow!("Error during installation: {e}"));
                    }
                    Err(e) => {
                        log_verbose(&format!("Task join error: {e}"));
                        return Err(anyhow::anyhow!("Task execution failed: {e}"));
                    }
                }
            }
        }
    }

    Ok(())
}

pub struct InstallService;

impl InstallService {
    pub async fn update_packages(
        action: PackageAction,
        specs: &[&str],
        workspace: Option<String>,
        ignore_scripts: bool,
        save_type: SaveType,
    ) -> Result<()> {
        log_verbose(&format!(
            "update packages: {:?} {:?} {:?} {:?}",
            action, specs, &workspace, ignore_scripts
        ));

        if specs.is_empty() {
            return Err(anyhow::anyhow!("No package specifications provided"));
        }

        let cwd = std::env::current_dir().context("Failed to get current directory")?;

        // Update working directory to project root (if in workspace)
        let root_path = crate::helper::workspace::update_cwd_to_root(&cwd).await?;

        // Update package.json and package-lock.json for all packages in batch
        update_package_json(&root_path, &action, specs, &workspace, &save_type)
            .await
            .context("Failed to update package.json")?;

        // Rebuild Deps
        crate::cmd::deps::build_deps(&root_path)
            .await
            .context("Failed to build package-lock.json")?;

        Self::install(ignore_scripts, &root_path)
            .await
            .context("Failed to install packages")?;

        Ok(())
    }

    pub async fn install(ignore_scripts: bool, root_path: &Path) -> Result<()> {
        // Package lock prerequisite check
        ensure_package_lock(root_path).await?;

        // load package-lock.json
        let package_lock_content = tokio::fs::read_to_string(root_path.join("package-lock.json"))
            .await
            .context("Failed to read package-lock.json")?;
        let package_lock: PackageLock = serde_json::from_str(&package_lock_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse package-lock.json: {}", e))?;

        let cache_dir = get_cache_dir();

        let groups = group_by_depth(&package_lock.packages);

        let mut depths: Vec<_> = groups.keys().cloned().collect();
        depths.sort_unstable();
        if !package_lock.packages.is_empty() {
            start_progress_bar();
            PROGRESS_BAR.set_length(package_lock.packages.len() as u64);
        }

        // Get the number of logical CPU cores of the system and set it to twice the number of CPU cores
        log_verbose(&format!("Setting concurrent limit to {CONCURRENT_LIMIT}"));
        let semaphore = Arc::new(Semaphore::new(CONCURRENT_LIMIT));

        install_packages(&groups, &cache_dir, root_path, semaphore)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to install packages: {}", e))?;

        finish_progress_bar("node_modules cloned");

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

    pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
        // Prepare global package directory and package.json
        let package_path = prepare_global_package_json(npm_spec, prefix)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to prepare global package.json: {}", e))?;

        log_verbose(&format!("Installing global package: {npm_spec}"));

        // Install dependencies
        Self::install(false, &package_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to install global package dependencies: {}", e))?;

        // Create package info from path
        let package_info = PackageInfo::from_path(&package_path)
            .await
            .context("Failed to create package info from path")?;

        // Get global bin directory using the common helper
        let target_bin_dir =
            get_global_bin_dir(prefix).context("Failed to get global bin directory")?;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

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

    #[test]
    fn test_extract_package_name_from_path() {
        // Test extracting package name from a standard path
        assert_eq!(extract_package_name("node_modules/lodash"), "lodash");

        // Test extracting package name from a nested path
        assert_eq!(
            extract_package_name("node_modules/parent/node_modules/child"),
            "child"
        );

        // Test extracting package name from a scoped package path
        assert_eq!(
            extract_package_name("node_modules/@scope/package"),
            "@scope/package"
        );
    }
}
