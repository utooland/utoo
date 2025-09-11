use anyhow::Result;
use std::path::PathBuf;

use crate::util::logger::{log_info, log_verbose};

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

    /// Install a package to the utoo cache directory using utoo's own installation logic
    /// This function is similar to prepare_global_package_json but installs to ~/.utoo/utx
    pub async fn install_package_to_cache(package_name: &str) -> Result<PathBuf> {
        // Parse package name and version
        let (name, version, _) = Self::parse_package_spec(package_name).await?;

        let cache_dir = Self::get_utoo_cache_dir()?;

        // Create a unique directory for this package installation
        let package_cache_dir = cache_dir.join(format!(
            "{}@{}",
            Self::package_name_to_dir_name(&name),
            version
        ));

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
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to resolve package '{}': {}",
                        name,
                        e
                    ));
                }
            }
        } else {
            version_spec
        };

        Ok((name, version, package_spec.to_string()))
    }
}
