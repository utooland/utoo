use anyhow::{Context, Result};
use tokio::fs;

use crate::util::logger::log_verbose;

pub async fn clean_package_lock() -> Result<()> {
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;
    let package_lock = current_dir.join("package-lock.json");
    let utoo_manifest = current_dir.join("node_modules/.utoo-manifest.json");

    if package_lock.exists() {
        fs::remove_file(&package_lock)
            .await
            .context("Failed to remove package-lock.json")?;
        log_verbose("package-lock.json removed successfully");
    }

    if utoo_manifest.exists() {
        fs::remove_file(&utoo_manifest)
            .await
            .context("Failed to remove .utoo-manifest.json")?;
        log_verbose(".utoo-manifest.json removed successfully");
    }

    Ok(())
}
