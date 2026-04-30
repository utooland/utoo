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
use utoo_ruborist::util::OnceMap;

use crate::util::json::read_json_file;

pub struct DiskManifestStore {
    cache_dir: PathBuf,
    /// Dedup `(name, resolved_version)` disk writes. The upstream
    /// `inflight_version<(name, original_spec)>` gate keys on the original
    /// spec, so two specs (`^4.17.0`, `~4.17.20`) resolving to the same
    /// `4.17.21` each call `store_version_manifest` and would otherwise
    /// spawn duplicate `write_json` tasks racing on truncate of the same
    /// path. `OnceMap::register` is a sync, atomic "first wins" check.
    inflight_version_writes: Arc<OnceMap<(String, String), ()>>,
}

impl DiskManifestStore {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            inflight_version_writes: Arc::new(OnceMap::new()),
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
        let key = (name.to_string(), version.to_string());
        let Some(notify) = self.inflight_version_writes.register(key.clone()) else {
            // Another caller already enqueued this write — skip the
            // duplicate serialize + spawn + truncate-race.
            return;
        };
        let path = self.manifest_path(name, version);
        // Move an `Arc` clone of the OnceMap into the task so we can
        // transition Waiting → Done after the write completes; this also
        // notifies any future `wait_if_pending` callers.
        let inflight = Arc::clone(&self.inflight_version_writes);
        tokio::spawn(async move {
            write_json(&path, &*manifest).await;
            inflight.complete(key, Some(()), notify);
        });
    }
}

/// Serialize `value` and write to `path`. On `NotFound`, create the parent
/// directory and retry once — saves the mkdir syscall on every warm-cache
/// rewrite. Errors are logged at debug; disk cache is opportunistic.
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
