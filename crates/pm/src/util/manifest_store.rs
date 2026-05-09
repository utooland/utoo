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

use crate::util::json::read_json_file;

// EXPERIMENT: ruborist_context swaps DiskManifestStore for NoopStore on
// this branch — type stays defined to keep the import path valid, but
// fields go unread.
#[allow(dead_code)]
pub struct DiskManifestStore {
    cache_dir: PathBuf,
}

#[allow(dead_code)]
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

/// Serialize `value` and write to `path`. On `NotFound`, create the parent
/// directory and retry once — saves the mkdir syscall on every warm-cache
/// rewrite. Errors are logged at debug; disk cache is opportunistic.
#[allow(dead_code)]
async fn write_json<T: Serialize>(path: &Path, value: &T) {
    let bytes = match serde_json::to_vec(value) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("Failed to serialize {path:?}: {e}");
            return;
        }
    };
    match crate::fs::write(path, &bytes).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent()
                && let Err(e) = crate::fs::create_dir_all(parent).await
            {
                tracing::debug!("Failed to create {parent:?}: {e}");
                return;
            }
            if let Err(e) = crate::fs::write(path, &bytes).await {
                tracing::debug!("Failed to write {path:?}: {e}");
            }
        }
        Err(e) => tracing::debug!("Failed to write {path:?}: {e}"),
    }
}
