use anyhow::Result;
use std::path::PathBuf;

use super::install::InstallService;
use crate::helper::lock::resolve_package_spec;

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

    /// Install a package to the utoo cache directory using utoo's own installation logic.
    /// Delegates to `InstallService::install_global_package` with a per-tool prefix
    /// (`~/.utoo/utx/<name>@<version>`), so the tool is installed as a dependency.
    pub async fn install_package_to_cache(package_name: &str) -> Result<PathBuf> {
        let (name, version, _) = resolve_package_spec(package_name).await?;

        let cache_dir = Self::get_utoo_cache_dir()?;
        let package_cache_dir = cache_dir.join(format!(
            "{}@{}",
            Self::package_name_to_dir_name(&name),
            version
        ));

        // Check if already installed
        if crate::fs::try_exists(&package_cache_dir.join("bin")).await? {
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
        tracing::debug!("Package {name} installed successfully");

        Ok(package_cache_dir)
    }
}
