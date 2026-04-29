//! Persistence backend for the registry's manifest cache.
//!
//! ruborist itself owns only the in-memory tier ([`super::cache::MemoryCache`]);
//! any persistent storage (disk, remote KV, …) is supplied by the host through
//! a [`ManifestStore`] implementation. This keeps the resolver free of file I/O
//! and lets hosts pick their own format, layout, and write strategy.
//!
//! Contract:
//! - `load_*` are awaited on the resolver hot path and gate ETag-validated
//!   fetches; they must return whatever the host has persisted, or `None`.
//! - `store_*` are fire-and-forget; implementations must return immediately
//!   and perform any writes asynchronously (the resolver does not await them).

use std::sync::Arc;

use async_trait::async_trait;

use super::cache::VersionsInfo;
use crate::model::manifest::CoreVersionManifest;

/// Persistence backend for the registry's manifest cache.
#[async_trait]
pub trait ManifestStore: Send + Sync {
    /// Load persisted versions info (used for ETag validation on cold start).
    async fn load_versions(&self, name: &str) -> Option<VersionsInfo>;

    /// Load a persisted version manifest (used for warm starts on non-semver
    /// registries, where every `(name, spec)` would otherwise require a
    /// network round-trip).
    async fn load_version_manifest(&self, name: &str, version: &str)
    -> Option<CoreVersionManifest>;

    /// Persist versions info. Fire-and-forget — must not block the caller.
    fn store_versions(&self, name: &str, info: Arc<VersionsInfo>);

    /// Persist a version manifest. Fire-and-forget — must not block the caller.
    fn store_version_manifest(&self, name: &str, version: &str, manifest: Arc<CoreVersionManifest>);
}

/// No-op store. Used when the host does not want any persistent cache.
pub struct NoopStore;

#[async_trait]
impl ManifestStore for NoopStore {
    async fn load_versions(&self, _name: &str) -> Option<VersionsInfo> {
        None
    }

    async fn load_version_manifest(
        &self,
        _name: &str,
        _version: &str,
    ) -> Option<CoreVersionManifest> {
        None
    }

    fn store_versions(&self, _name: &str, _info: Arc<VersionsInfo>) {}

    fn store_version_manifest(
        &self,
        _name: &str,
        _version: &str,
        _manifest: Arc<CoreVersionManifest>,
    ) {
    }
}
