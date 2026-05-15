//! Manifest provider boundary for resolver drivers.
//!
//! The demand BFS loop owns per-run cache, waiters, and inflight de-duplication.
//! A provider only executes one manifest job and hides whether it satisfied the
//! job from memory, disk/OPFS, or the network.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use super::cache::VersionsInfo;
use super::manifest::MetadataFormat;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::traits::registry::RegistryClient;

/// Full-manifest data returned by a provider job.
#[derive(Clone)]
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

/// Unit of work spawned by the demand BFS loop.
#[derive(Clone)]
pub enum ManifestJob {
    Full {
        name: String,
        /// Optional range/tag from the BFS edge that caused this full-manifest
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
        /// Metadata format for the version endpoint. Semver-capable registries
        /// accept install-v1 for range/tag queries; npmjs exact-version
        /// fallback requires the complete metadata format.
        format: MetadataFormat,
    },
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

/// Lower-level manifest provider used by the demand BFS loop.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ManifestProvider: RegistryClient + Clone + Send + Sync + 'static {
    /// Execute one manifest job. The provider owns I/O, persistence, and
    /// parse/extract offloading; scheduling, waiters, and inflight
    /// de-duplication stay in the BFS loop.
    async fn execute_manifest_job(&self, job: ManifestJob) -> Result<ManifestJobDone, Self::Error>;
}

/// Raw full-manifest bytes fetched by a provider before parsing.
pub(crate) enum ProviderFullManifestBytes {
    Fresh { bytes: Bytes, etag: Option<String> },
    NotModified { versions: Arc<VersionsInfo> },
}
