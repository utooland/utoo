use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::util::save_type::{PackageAction, SaveType};
use crate::util::logger::{log_info, log_verbose};
use crate::helper::lock::{ensure_package_lock, group_by_depth, prepare_global_package_json, update_package_json};
use crate::helper::workspace::update_cwd_to_root;
use crate::helper::global_bin::get_global_bin_dir;
use crate::model::package::PackageInfo;
use crate::service::build::BuildService;

pub struct PackageManagementService;

impl PackageManagementService {
    pub async fn update_packages(
        action: PackageAction,
        specs: &[&str],
        workspace: Option<String>,
        save_type: SaveType,
        is_global: bool,
    ) -> Result<()> {
        if is_global {
            Self::handle_global_packages(action, specs, save_type).await
        } else {
            Self::handle_local_packages(action, specs, workspace, save_type).await
        }
    }

    async fn handle_global_packages(
        action: PackageAction,
        specs: &[&str],
        save_type: SaveType,
    ) -> Result<()> {
        let global_bin_dir = get_global_bin_dir(&None)?;
        
        match action {
            PackageAction::Add => {
                // For global installs, process packages one by one
                for spec in specs {
                    // Create a unique directory for this package installation
                    let package_cache_dir = global_bin_dir.join(spec);
                    tokio::fs::create_dir_all(&package_cache_dir).await?;
                    
                    // Prepare global package JSON for this specific package
                    let global_package_json = prepare_global_package_json(spec, None).await?;
                    
                    // Install this package
                    let single_spec = vec![*spec];
                    Self::install_global_packages(&single_spec, save_type, &global_package_json, &global_bin_dir).await?;
                }
                Ok(())
            }
            PackageAction::Remove => {
                // For global removals, process packages one by one
                for spec in specs {
                    // Remove this package
                    Self::remove_global_packages(&[spec], &global_bin_dir.join(spec), &global_bin_dir).await?;
                }
                Ok(())
            }
        }
    }

    async fn handle_local_packages(
        action: PackageAction,
        specs: &[&str],
        workspace: Option<String>,
        save_type: SaveType,
    ) -> Result<()> {
        let cwd = std::env::current_dir()?;
        update_cwd_to_root(&cwd).await?;
        
        match action {
            PackageAction::Add => {
                if specs.is_empty() {
                    Self::install_from_package_json(workspace).await
                } else {
                    Self::install_specific_packages(specs, workspace, save_type).await
                }
            }
            PackageAction::Remove => {
                Self::remove_local_packages(specs, workspace).await
            }
        }
    }

    async fn install_from_package_json(workspace: Option<String>) -> Result<()> {
        log_info("Installing from package.json...");
        let cwd = std::env::current_dir()?;
        ensure_package_lock(&cwd).await?;
        BuildService::build_deps(workspace).await?;
        BuildService::rebuild().await
    }

    async fn install_specific_packages(
        specs: &[&str],
        workspace: Option<String>,
        save_type: SaveType,
    ) -> Result<()> {
        log_info(&format!("Installing {} packages...", specs.len()));
        
        // Update package.json with new dependencies
        let cwd = std::env::current_dir()?;
        for spec in specs {
            update_package_json(&cwd, &PackageAction::Add, &[spec], &workspace, &save_type).await?;
        }

        // Install all dependencies
        BuildService::build_deps(workspace).await?;
        BuildService::rebuild().await
    }

    async fn install_global_packages(
        specs: &[&str],
        save_type: SaveType,
        package_json_path: &Path,
        global_bin_dir: &Path,
    ) -> Result<()> {
        log_info(&format!("Installing {} global packages...", specs.len()));

        // Update global package.json
        for spec in specs {
            update_package_json(package_json_path, &PackageAction::Add, &[spec], &None, &save_type).await?;
        }

        // Create package-lock.json for global installation
        ensure_package_lock(package_json_path).await?;

        // Install packages
        let package_lock_data = crate::util::json::load_package_lock_json_from_path(package_json_path)?;
        let package_lock: crate::helper::lock::PackageLock = serde_json::from_value(package_lock_data)?;

        let grouped_packages = group_by_depth(&package_lock.packages);
        let cache_dir = crate::util::cache::get_cache_dir();
        let cwd = std::env::current_dir()?;
        let semaphore = Arc::new(Semaphore::new(100));
        crate::service::install::install_packages(&grouped_packages, &cache_dir, &cwd, semaphore).await.map_err(|e| anyhow::anyhow!("{}", e))?;

        // Link global binaries
        for (_, packages) in grouped_packages {
            for (path, package) in packages {
                if let Some(name) = &package.name {
                    let package_info = PackageInfo::from_path(Path::new(&path))?;
                    package_info.link_to_global(global_bin_dir).await?;
                    log_verbose(&format!("Linked global binary for {}", name));
                }
            }
        }

        Ok(())
    }

    async fn remove_local_packages(
        specs: &[&str],
        workspace: Option<String>,
    ) -> Result<()> {
        log_info(&format!("Removing {} packages...", specs.len()));

        // Remove from package.json
        for spec in specs {
            Self::remove_package_from_json(spec, workspace.as_deref()).await?;
        }

        // Reinstall remaining dependencies
        BuildService::build_deps(workspace).await?;
        BuildService::rebuild().await
    }

    async fn remove_global_packages(
        specs: &[&str],
        _package_json_path: &Path,
        global_bin_dir: &Path,
    ) -> Result<()> {
        log_info(&format!("Removing {} global packages...", specs.len()));

        // Remove from global package.json and unlink binaries
        for spec in specs {
            Self::remove_package_from_json(spec, None).await?;
            Self::unlink_global_binary(spec, global_bin_dir)?;
        }

        Ok(())
    }

    async fn remove_package_from_json(
        spec: &str,
        _workspace: Option<&str>,
    ) -> Result<()> {
        // Extract package name from spec (e.g., "lodash@4.17.21" -> "lodash")
        let package_name = spec.split('@').next().unwrap_or(spec);
        
        // TODO: Implement package.json removal logic
        log_verbose(&format!("Removing {} from package.json", package_name));
        Ok(())
    }

    fn unlink_global_binary(spec: &str, global_bin_dir: &Path) -> Result<()> {
        let package_name = spec.split('@').next().unwrap_or(spec);
        let binary_path = global_bin_dir.join(package_name);
        
        if binary_path.exists() {
            std::fs::remove_file(&binary_path)
                .with_context(|| format!("Failed to remove global binary for {}", package_name))?;
            log_verbose(&format!("Unlinked global binary for {}", package_name));
        }
        
        Ok(())
    }

    /// Get the utoo cache directory (~/.utoo/utx)
    pub fn get_utoo_cache_dir() -> Result<std::path::PathBuf> {
        let home_dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Unable to find home directory"))?;
        Ok(home_dir.join(".utoo").join("utx"))
    }

    /// Convert package name to safe directory name
    /// Examples:
    /// - "cowsay" -> "cowsay"
    /// - "@modelcontextprotocol/create-server" -> "@modelcontextprotocol_create-server"
    fn package_name_to_dir_name(package_name: &str) -> String {
        package_name.replace("/", "_")
    }

    /// Install a package to the utoo cache directory using utoo's own installation logic
    /// This function is similar to prepare_global_package_json but installs to ~/.utoo/utx
    pub async fn install_package_to_cache(package_name: &str) -> Result<std::path::PathBuf> {
        // Parse package name and version
        let (name, version, _) = Self::parse_package_spec(package_name).await?;

        let cache_dir = Self::get_utoo_cache_dir()?;

        // Create a unique directory for this package installation
        let package_cache_dir =
            cache_dir.join(format!("{}@{}", Self::package_name_to_dir_name(&name), version));

        // Maybe the package is already installed
        if package_cache_dir.join("bin").exists() {
            log_verbose(&format!(
                "Package {} already cached at {}",
                name,
                package_cache_dir.display()
            ));
            return Ok(package_cache_dir);
        }

        log_info(&format!("Installing package {name} to cache using utoo..."));
        crate::cmd::install::install_global_package(
            package_name,
            Some(package_cache_dir.to_string_lossy().to_string().as_str()),
        )
        .await?;
        log_info(&format!("Package {name} installed successfully using utoo"));

        Ok(package_cache_dir)
    }

    /// Parse a package spec and resolve the latest version
    async fn parse_package_spec(package_spec: &str) -> Result<(String, String, String)> {
        let (name, version_spec) = crate::util::cache::parse_pattern(package_spec);
        
        // If no version specified, resolve latest
        let version = if version_spec == "*" {
            // Resolve the latest version
            match crate::util::registry::resolve(&name, "*").await {
                Ok(resolved) => resolved.version,
                Err(e) => return Err(anyhow::anyhow!("Failed to resolve package '{}': {}", name, e)),
            }
        } else {
            version_spec
        };

        Ok((name, version, package_spec.to_string()))
    }
}
