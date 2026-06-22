use crate::fs;
use anyhow::{Context, Result};
use std::path::Path;

pub async fn clean_package_lock(cwd: &Path) -> Result<()> {
    let package_lock = cwd.join("package-lock.json");

    if crate::fs::try_exists(&package_lock).await? {
        fs::remove_file(&package_lock)
            .await
            .context("Failed to remove package-lock.json")?;
        tracing::debug!("package-lock.json removed successfully");
    }

    Ok(())
}
