use anyhow::Result;
use std::path::{Path, PathBuf};

use super::install::InstallService;
use crate::helper::lock::resolve_package_spec;
use crate::util::process_lock;

/// Package management service for handling package installation and caching
pub struct PackageManagementService;

impl PackageManagementService {
    /// Get the utoo cache directory (~/.utoo/utx)
    pub fn get_utoo_cache_dir() -> Result<PathBuf> {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Unable to find home directory"))?;
        Ok(home_dir.join(".utoo").join("utx"))
    }

    /// Convert package name to safe directory name
    /// Examples:
    /// - "cowsay" -> "cowsay"
    /// - "@modelcontextprotocol/create-server" -> "@modelcontextprotocol_create-server"
    fn package_name_to_dir_name(package_name: &str) -> String {
        package_name.replace("/", "_")
    }

    async fn write_install_complete_marker(package_cache_dir: &Path) -> Result<()> {
        let marker = package_cache_dir.join(".utoo-install-complete");
        let temp_marker = package_cache_dir.join(".utoo-install-complete.tmp");

        crate::fs::write(&temp_marker, b"ok").await?;
        crate::fs::rename(&temp_marker, &marker).await?;

        Ok(())
    }

    /// Install a package to the utoo cache directory using utoo's own installation logic
    /// This function is similar to prepare_global_package_json but installs to ~/.utoo/utx
    pub async fn install_package_to_cache(package_name: &str) -> Result<PathBuf> {
        let (name, version, _) = resolve_package_spec(package_name).await?;

        let cache_dir = Self::get_utoo_cache_dir()?;
        let package_cache_dir = cache_dir.join(format!(
            "{}@{}",
            Self::package_name_to_dir_name(&name),
            version
        ));

        let complete_marker = package_cache_dir.join(".utoo-install-complete");

        // Check if already installed.  The `bin/` directory is created before
        // binaries are fully linked, so only the explicit marker means that
        // the install finished successfully.
        if crate::fs::try_exists(&complete_marker).await? {
            tracing::debug!(
                "Package {} already cached at {}",
                name,
                package_cache_dir.display()
            );
            return Ok(package_cache_dir);
        }

        let lock_path = process_lock::lock_path_for(&package_cache_dir, ".install-lock");
        let _lock = process_lock::lock_exclusive(&lock_path).await?;

        if crate::fs::try_exists(&complete_marker).await? {
            tracing::debug!(
                "Package {} already cached at {}",
                name,
                package_cache_dir.display()
            );
            return Ok(package_cache_dir);
        }

        tracing::debug!("Installing package {name} to cache...");
        InstallService::install_global_package(
            package_name,
            Some(package_cache_dir.to_string_lossy().into_owned().as_str()),
        )
        .await?;
        Self::write_install_complete_marker(&package_cache_dir).await?;
        tracing::debug!("Package {name} installed successfully");

        Ok(package_cache_dir)
    }
}
