//! Disk persistence for ruborist's project-level manifest cache.
//!
//! Stored at `<root>/node_modules/.utoo-manifest.json`. Used to skip the
//! preload phase on warm installs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use utoo_ruborist::service::ProjectCacheData;

pub fn path_for(root: &Path) -> PathBuf {
    root.join("node_modules/.utoo-manifest.json")
}

pub async fn load(root: &Path) -> ProjectCacheData {
    let path = path_for(root);
    if !tokio_fs_ext::try_exists(&path).await.unwrap_or(false) {
        return ProjectCacheData::default();
    }
    match tokio_fs_ext::read_to_string(&path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::debug!("Failed to parse project cache at {}: {e}", path.display());
            ProjectCacheData::default()
        }),
        Err(e) => {
            tracing::debug!("Failed to read project cache at {}: {e}", path.display());
            ProjectCacheData::default()
        }
    }
}

pub async fn save(root: &Path, data: &ProjectCacheData) -> Result<()> {
    if data.cache.is_empty() {
        return Ok(());
    }
    let path = path_for(root);
    if let Some(parent) = path.parent() {
        tokio_fs_ext::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec(data).context("Failed to serialize project cache")?;
    tokio_fs_ext::write(&path, &bytes)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
