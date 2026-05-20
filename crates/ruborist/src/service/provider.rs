//! Manifest provider boundary.
//!
//! A provider executes one concrete manifest job: persistent-metadata read,
//! registry I/O, JSON parse off-runtime, or single-version extraction from a
//! full manifest. BFS ordering, waiters, and single-flight de-duplication are
//! driver responsibilities, not provider responsibilities.
//!
//! [`UnifiedRegistry`] is the existing in-process driver that still owns that
//! scheduling state.
//!
//! [`UnifiedRegistry`]: crate::service::UnifiedRegistry

use std::sync::Arc;

use async_trait::async_trait;

use super::cache::VersionsInfo;
use super::manifest::MetadataFormat;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::traits::registry::RegistryClient;

/// Full-manifest data returned by a provider job.
#[derive(Clone)]
pub enum ManifestFullData {
    /// Fresh full manifest bytes were fetched and parsed. When the full job
    /// carried the triggering spec, the provider may also return the matching
    /// version manifest extracted in the same parse worker.
    Full {
        manifest: Arc<FullManifest>,
        speculative: Option<(String, Arc<CoreVersionManifest>)>,
    },
    /// The registry returned 304 and the persisted version list is valid.
    Versions(Arc<VersionsInfo>),
}

/// Unit of work a driver dispatches to a provider.
#[derive(Clone)]
pub enum ManifestJob {
    /// Fetch or validate a package full manifest.
    Full {
        name: String,
        /// Optional range/tag from the BFS edge that caused this full-manifest
        /// fetch. Providers can use it to speculatively extract the current
        /// version while full-manifest bytes are already on a CPU worker.
        spec: Option<String>,
    },
    /// Fetch a version manifest.
    Version {
        name: String,
        /// Driver-side cache and waiter key. Never sent on the wire.
        spec: String,
        /// What the registry request actually carries. Equal to `spec` in
        /// the common case; differs when the driver has already resolved a
        /// range to an exact version against a cached version list (the
        /// npmjs 304 path, where `spec` is the original range and
        /// `fetch_spec` is the picked exact version).
        fetch_spec: String,
        format: MetadataFormat,
    },
    /// Extract one exact version from an already fetched full manifest.
    ExtractVersion {
        name: String,
        spec: String,
        version: String,
        full: Arc<FullManifest>,
    },
}

/// Result of one provider job.
pub enum ManifestJobDone {
    Full {
        name: String,
        data: ManifestFullData,
    },
    Version {
        name: String,
        spec: String,
        manifest: Arc<CoreVersionManifest>,
    },
}

/// Manifest fetch interface a driver dispatches [`ManifestJob`]s through.
///
/// Implementors must be cheaply cloneable and shareable across worker tasks.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ManifestProvider: RegistryClient + Clone + Send + Sync + 'static {
    /// Execute a single manifest job. The provider owns the per-job work
    /// (I/O, parse, store read/write); the caller owns scheduling, the
    /// per-key waiter fan-out, and single-flight de-duplication across
    /// overlapping jobs for the same key.
    async fn execute_manifest_job(&self, job: ManifestJob) -> Result<ManifestJobDone, Self::Error>;
}
