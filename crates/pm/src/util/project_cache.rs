//! Disk persistence for ruborist's project-level manifest cache.
//!
//! Stored at `<root>/node_modules/.utoo-manifest.json`. Seeds the demand
//! resolver's manifest cache so warm installs skip re-fetching.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use utoo_ruborist::service::ProjectCacheData;

use crate::util::json::read_json_file;

pub fn path_for(root: &Path) -> PathBuf {
    root.join("node_modules/.utoo-manifest.json")
}

pub async fn load(root: &Path) -> ProjectCacheData {
    read_json_file(&path_for(root)).await.unwrap_or_default()
}

pub async fn save(root: &Path, data: &ProjectCacheData) -> Result<()> {
    if data.cache.is_empty() {
        return Ok(());
    }
    let path = path_for(root);
    if let Some(parent) = path.parent() {
        crate::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create {parent:?}"))?;
    }
    let bytes = serde_json::to_vec(data).context("Failed to serialize project cache")?;
    crate::fs::write(&path, &bytes)
        .await
        .with_context(|| format!("Failed to write {path:?}"))
}
