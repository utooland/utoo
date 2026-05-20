//! Disk-backed [`ManifestStore`] for the package manager.
//!
//! Layout:
//! - `<cache_dir>/<name>/versions.json`              ← `VersionsInfo` (etag + version list)
//! - `<cache_dir>/<name>/manifests/<version>.json`   ← `CoreVersionManifest`
//!
//! Writes are fire-and-forget: each `store_*` call enqueues a background write
//! and returns immediately, so the resolver hot path never waits on the disk.
//! Errors are logged at debug level — disk cache is opportunistic; a failed
//! write only costs a future cache miss.
//! Serialization and file writes run on a dedicated writer thread so manifest
//! persistence does not occupy async runtime workers or Tokio's blocking pool.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::JoinHandle;

use async_trait::async_trait;
use serde::Serialize;
use utoo_ruborist::model::manifest::CoreVersionManifest;
use utoo_ruborist::service::{ManifestStore, VersionsInfo};

use crate::util::json::{read_json_file, write_compact_sync};

/// Opportunistic writer backlog. If disk stalls beyond this, new cache writes
/// are dropped instead of letting resolver memory grow without bound.
const MANIFEST_WRITE_QUEUE_CAPACITY: usize = 1024;

pub struct DiskManifestStore {
    cache_dir: PathBuf,
    writer: Option<ManifestWriter>,
}

impl DiskManifestStore {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            writer: Some(ManifestWriter::spawn()),
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

    fn enqueue_write(&self, job: ManifestWriteJob) {
        if let Some(writer) = &self.writer {
            writer.enqueue(job);
        }
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
        self.enqueue_write(ManifestWriteJob::Versions { path, info });
    }

    fn store_version_manifest(
        &self,
        name: &str,
        version: &str,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let path = self.manifest_path(name, version);
        self.enqueue_write(ManifestWriteJob::VersionManifest { path, manifest });
    }
}

impl Drop for DiskManifestStore {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            writer.join();
        }
    }
}

enum ManifestWriteJob {
    Versions {
        path: PathBuf,
        info: Arc<VersionsInfo>,
    },
    VersionManifest {
        path: PathBuf,
        manifest: Arc<CoreVersionManifest>,
    },
}

struct ManifestWriter {
    tx: SyncSender<ManifestWriteJob>,
    handle: JoinHandle<()>,
}

impl ManifestWriter {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::sync_channel(MANIFEST_WRITE_QUEUE_CAPACITY);
        let handle = std::thread::Builder::new()
            .name("utoo-manifest-store".to_string())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        ManifestWriteJob::Versions { path, info } => {
                            write_json_sync(&path, &*info);
                        }
                        ManifestWriteJob::VersionManifest { path, manifest } => {
                            write_json_sync(&path, &*manifest);
                        }
                    }
                }
            })
            .expect("failed to spawn manifest store writer");
        Self { tx, handle }
    }

    fn enqueue(&self, job: ManifestWriteJob) {
        match self.tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::debug!("Manifest store writer queue full; dropping cache write");
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::debug!("Manifest store writer stopped before accepting write");
            }
        }
    }

    fn join(self) {
        drop(self.tx);
        if self.handle.join().is_err() {
            tracing::debug!("Manifest store writer panicked");
        }
    }
}

/// Apply the manifest-cache write policy on top of
/// [`crate::util::json::write_compact_sync`]: on `NotFound`, create the
/// parent directory once and retry — this is how the resolver hot path
/// avoids the up-front `mkdir` syscall on every warm-cache rewrite. All
/// errors are swallowed at the `debug` log level because the disk cache is
/// opportunistic; a dropped write only costs a future cache miss.
fn write_json_sync<T: Serialize>(path: &Path, value: &T) {
    match write_compact_sync(path, value) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if let Some(parent) = path.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                tracing::debug!("Failed to create {parent:?}: {e}");
                return;
            }
            if let Err(e) = write_compact_sync(path, value) {
                tracing::debug!("Failed to write {path:?}: {e}");
            }
        }
        Err(e) => tracing::debug!("Failed to write {path:?}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tempfile::tempdir;
    use utoo_ruborist::service::Versions;

    use super::*;

    #[test]
    fn drop_flushes_queued_manifest_writes() {
        let dir = tempdir().unwrap();
        {
            let store = DiskManifestStore::new(dir.path().to_path_buf());
            store.store_versions(
                "pkg",
                Arc::new(VersionsInfo {
                    versions: Versions {
                        version_list: vec!["1.0.0".to_string()],
                        dist_tags: HashMap::from([("latest".to_string(), "1.0.0".to_string())]),
                    },
                    etag: Some("etag".to_string()),
                    last_updated: 1,
                }),
            );
            store.store_version_manifest(
                "pkg",
                "1.0.0",
                Arc::new(CoreVersionManifest {
                    name: "pkg".to_string(),
                    version: "1.0.0".to_string(),
                    ..Default::default()
                }),
            );
        }

        let versions: VersionsInfo =
            serde_json::from_slice(&std::fs::read(dir.path().join("pkg/versions.json")).unwrap())
                .unwrap();
        assert_eq!(versions.versions.version_list, ["1.0.0"]);

        let manifest: CoreVersionManifest = serde_json::from_slice(
            &std::fs::read(dir.path().join("pkg/manifests/1.0.0.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.name, "pkg");
        assert_eq!(manifest.version, "1.0.0");
    }
}
