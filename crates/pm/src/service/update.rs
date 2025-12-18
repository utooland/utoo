use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;

pub async fn clean_package_lock(cwd: &Path) -> Result<()> {
    let package_lock = cwd.join("package-lock.json");
    let utoo_manifest = cwd.join("node_modules/.utoo-manifest.json");

    if tokio::fs::try_exists(&package_lock).await? {
        fs::remove_file(&package_lock)
            .await
            .context("Failed to remove package-lock.json")?;
        tracing::debug!("package-lock.json removed successfully");
    }

    if tokio::fs::try_exists(&utoo_manifest).await? {
        fs::remove_file(&utoo_manifest)
            .await
            .context("Failed to remove .utoo-manifest.json")?;
        tracing::debug!(".utoo-manifest.json removed successfully");
    }

    Ok(())
}
