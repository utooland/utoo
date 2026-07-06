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

    /// Install a package to the utoo cache directory using utoo's own installation logic.
    /// Delegates to `InstallService::install_global_package` with a per-tool prefix
    /// (`~/.utoo/utx/<name>/<version>`), so the tool is installed as a dependency.
    ///
    /// The prefix uses the same `<name>/<version>` two-segment layout as the
    /// package store (`~/.cache/nm`): a scoped name nests naturally
    /// (`@scope/pkg` → `@scope/pkg/<version>`) so no name escaping is needed.
    /// The directory is purely an internal addressing key — nothing parses it
    /// back (see `execute.rs`, which only searches under the returned path).
    pub async fn install_package_to_cache(package_name: &str) -> Result<PathBuf> {
        let (name, version, _) = resolve_package_spec(package_name).await?;

        let cache_dir = Self::get_utoo_cache_dir()?;
        let package_cache_dir = cache_dir.join(&name).join(&version);

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
        // `ut x` exposes no per-invocation script flags, so the policy comes
        // from global config only (resolved inside `install_global_package`).
        InstallService::install_global_package(
            package_name,
            Some(package_cache_dir.to_string_lossy().into_owned().as_str()),
            &crate::util::script_policy::ScriptPolicyArgs::default(),
        )
        .await?;
        tracing::debug!("Package {name} installed successfully");

        Ok(package_cache_dir)
    }
}
