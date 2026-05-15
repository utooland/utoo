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
//! Serialization and file writes run on Tokio's blocking pool so manifest
//! persistence does not occupy async runtime workers that are driving network IO.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use utoo_ruborist::model::manifest::CoreVersionManifest;
use utoo_ruborist::service::{ManifestStore, VersionsInfo};

use crate::util::json::read_json_file;

pub struct DiskManifestStore {
    cache_dir: PathBuf,
    load_enabled: bool,
}

impl DiskManifestStore {
    pub fn new(cache_dir: PathBuf) -> Self {
        let load_enabled = cache_dir.exists();
        Self {
            cache_dir,
            load_enabled,
        }
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
        if !self.load_enabled {
            return None;
        }
        read_json_file(&self.versions_path(name)).await.ok()
    }

    async fn load_version_manifest(
        &self,
        name: &str,
        version: &str,
    ) -> Option<CoreVersionManifest> {
        if !self.load_enabled {
            return None;
        }
        read_json_file(&self.manifest_path(name, version))
            .await
            .ok()
    }

    fn store_versions(&self, name: &str, info: Arc<VersionsInfo>) {
        let path = self.versions_path(name);
        tokio::task::spawn_blocking(move || write_json_sync(&path, &*info));
    }

    fn store_version_manifest(
        &self,
        name: &str,
        version: &str,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let path = self.manifest_path(name, version);
        tokio::task::spawn_blocking(move || write_json_sync(&path, &*manifest));
    }
}

/// Serialize `value` and write to `path`. On `NotFound`, create the parent
/// directory and retry once — saves the mkdir syscall on every warm-cache
/// rewrite. Errors are logged at debug; disk cache is opportunistic.
fn write_json_sync<T: Serialize>(path: &Path, value: &T) {
    let bytes = match serde_json::to_vec(value) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("Failed to serialize {path:?}: {e}");
            return;
        }
    };
    match std::fs::write(path, &bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                tracing::debug!("Failed to create {parent:?}: {e}");
                return;
            }
            if let Err(e) = std::fs::write(path, &bytes) {
                tracing::debug!("Failed to write {path:?}: {e}");
            }
        }
        Err(e) => tracing::debug!("Failed to write {path:?}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoo_ruborist::service::{Versions, VersionsInfo};

    #[tokio::test]
    async fn cold_start_store_skips_read_misses_until_next_process() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("missing-cache");
        let store = DiskManifestStore::new(cache_dir.clone());

        let versions = VersionsInfo {
            versions: Versions {
                version_list: vec!["1.0.0".to_string()],
                dist_tags: Default::default(),
            },
            etag: Some("etag".to_string()),
            last_updated: 1,
        };
        let versions_path = cache_dir.join("pkg").join("versions.json");
        write_json_sync(&versions_path, &versions);

        assert!(store.load_versions("pkg").await.is_none());

        let warm_store = DiskManifestStore::new(cache_dir);
        assert!(warm_store.load_versions("pkg").await.is_some());
    }
}
