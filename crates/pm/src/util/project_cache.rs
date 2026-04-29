//! Disk persistence for ruborist's project-level manifest cache.
//!
//! Stored at `<root>/node_modules/.utoo-manifest.json`. Used to skip the
//! preload phase on warm installs.

use std::path::{Path, PathBuf};

use anyhow::Result;
use utoo_ruborist::service::ProjectCacheData;

use crate::util::json::{read_json_file, write_json_file};

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
    write_json_file(&path_for(root), data).await
}
