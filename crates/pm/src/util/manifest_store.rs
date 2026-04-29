//! Disk-backed [`ManifestStore`] for the package manager.
//!
//! Layout:
//! - `<cache_dir>/<name>/versions.json`              ← `VersionsInfo` (etag + version list)
//! - `<cache_dir>/<name>/manifests/<version>.json`   ← `CoreVersionManifest`
//!
//! Writes are fire-and-forget: each `store_*` call spawns a detached task and
//! returns immediately, so the resolver hot path never waits on the disk.
//! Errors are logged at debug level — disk cache is opportunistic; a failed
//! write only costs a future cache miss.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use utoo_ruborist::model::manifest::CoreVersionManifest;
use utoo_ruborist::service::{ManifestStore, VersionsInfo};

pub struct DiskManifestStore {
    cache_dir: PathBuf,
}

impl DiskManifestStore {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    fn versions_path(&self, name: &str) -> PathBuf {
        self.cache_dir.join(name).join("versions.json")
    }

    fn manifest_path(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir
            .join(name)
            .join("manifests")
            .join(format!("{version}.json"))
    }
}

#[async_trait]
impl ManifestStore for DiskManifestStore {
    async fn load_versions(&self, name: &str) -> Option<VersionsInfo> {
        read_json(&self.versions_path(name)).await
    }

    async fn load_version_manifest(
        &self,
        name: &str,
        version: &str,
    ) -> Option<CoreVersionManifest> {
        read_json(&self.manifest_path(name, version)).await
    }

    fn store_versions(&self, name: &str, info: Arc<VersionsInfo>) {
        let path = self.versions_path(name);
        tokio::spawn(async move { write_json(&path, &*info).await });
    }

    fn store_version_manifest(
        &self,
        name: &str,
        version: &str,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let path = self.manifest_path(name, version);
        tokio::spawn(async move { write_json(&path, &*manifest).await });
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    if !tokio_fs_ext::try_exists(path).await.unwrap_or(false) {
        return None;
    }
    match tokio_fs_ext::read_to_string(path).await {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::debug!("Failed to parse {}: {e}", path.display());
                None
            }
        },
        Err(e) => {
            tracing::debug!("Failed to read {}: {e}", path.display());
            None
        }
    }
}

async fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent()
        && let Err(e) = tokio_fs_ext::create_dir_all(parent).await
    {
        tracing::debug!("disk cache mkdir failed {}: {e}", parent.display());
        return;
    }
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::debug!("disk cache serialize failed {}: {e}", path.display());
            return;
        }
    };
    if let Err(e) = tokio_fs_ext::write(path, &bytes).await {
        tracing::debug!("disk cache write failed {}: {e}", path.display());
    }
}
