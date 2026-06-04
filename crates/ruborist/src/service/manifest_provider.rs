//! Manifest provider boundary for resolver drivers.
//!
//! The demand loop owns per-run cache, waiters, and inflight de-duplication.
//! A provider only executes one manifest job and hides whether it satisfied the
//! job from memory, disk/OPFS, or the network.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use super::cache::VersionsInfo;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::traits::registry::RegistryClient;

/// Full-manifest data returned by a provider job.
#[derive(Debug, Clone)]
pub enum ManifestFullData {
    /// A parsed full manifest. When the original job carried a spec, the
    /// provider may also return the matching version manifest extracted in the
    /// same worker task so the main loop can avoid an extra extract hop.
    Full {
        manifest: Arc<FullManifest>,
        speculative: Option<(String, Arc<CoreVersionManifest>)>,
    },
    /// A validated versions list, usually from a 304 path. The main loop can
    /// resolve a concrete version and schedule a version-manifest job.
    Versions(Arc<VersionsInfo>),
}

/// Unit of work executed by the demand loop.
#[derive(Debug, Clone)]
pub enum ManifestJob {
    Full {
        name: String,
        /// Optional range/tag from the edge that caused this full-manifest
        /// fetch. The provider can use it to speculatively extract the current
        /// version while the full manifest bytes are already on a CPU worker.
        spec: Option<String>,
    },
    Version {
        name: String,
        /// Cache/waiter key owned by the main loop.
        spec: String,
        /// Registry request spec. For npmjs 304 flows this is the resolved
        /// exact version, while `spec` remains the original range key.
        fetch_spec: String,
    },
    ExtractVersion {
        name: String,
        spec: String,
        version: String,
        full: Arc<FullManifest>,
    },
}

/// Result of one provider job.
#[derive(Debug, Clone)]
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

/// Lower-level manifest provider used by the demand loop.
///
/// The driver clones the provider for each job it issues, so implementors
/// should keep `Clone` cheap (for example by storing shared state behind `Arc`).
///
/// On native the driver runs each fetch job as a `tokio::spawn`ed task, so the
/// provider is `Send + Sync` and its job futures are `Send` — fetch + parse for
/// independent manifests run in parallel across the multi-threaded runtime. On
/// wasm32 there are no threads, so the provider is `?Send` and jobs run via
/// `spawn_local`.
///
/// CPU-bound work inside a job (manifest JSON parsing, version extraction) is
/// offloaded by the implementor: on native the `*_off_runtime` helpers use
/// `rayon` plus a oneshot back-channel; on wasm32 the same work runs inline.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ManifestProvider: RegistryClient + Clone + Send + Sync + 'static {
    /// Execute one manifest job. The provider owns I/O, persistence, and
    /// parse/extract offloading; scheduling, waiters, and inflight
    /// de-duplication stay in the demand loop.
    async fn execute_manifest_job(&self, job: ManifestJob) -> Result<ManifestJobDone, Self::Error>;
}

/// Raw full-manifest bytes fetched by a provider before parsing.
#[derive(Debug, Clone)]
pub(crate) enum ProviderFullManifestBytes {
    Fresh { bytes: Bytes, etag: Option<String> },
    NotModified { versions: Arc<VersionsInfo> },
}
