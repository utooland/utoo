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

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use utoo_ruborist::model::manifest::CoreVersionManifest;
use utoo_ruborist::service::{ManifestStore, VersionsInfo};

use crate::util::json::{read_json_file, write_json_file};

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
        read_json_file(&self.versions_path(name)).await.ok()
    }

    async fn load_version_manifest(
        &self,
        name: &str,
        version: &str,
    ) -> Option<CoreVersionManifest> {
        read_json_file(&self.manifest_path(name, version))
            .await
            .ok()
    }

    fn store_versions(&self, name: &str, info: Arc<VersionsInfo>) {
        let path = self.versions_path(name);
        tokio::spawn(async move {
            if let Err(e) = write_json_file(&path, &*info).await {
                tracing::debug!("Failed to write {path:?}: {e}");
            }
        });
    }

    fn store_version_manifest(
        &self,
        name: &str,
        version: &str,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let path = self.manifest_path(name, version);
        tokio::spawn(async move {
            if let Err(e) = write_json_file(&path, &*manifest).await {
                tracing::debug!("Failed to write {path:?}: {e}");
            }
        });
    }
}
