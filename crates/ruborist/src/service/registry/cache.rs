//! Manifest cache data types for dependency resolution.
//!
//! ruborist owns no resolution-time cache state — the demand resolver holds the
//! per-run manifest store, and persistent storage (disk, remote KV, …) is
//! delegated to a [`super::store::ManifestStore`] supplied by the host. This
//! module holds the shared, pure-data types: [`VersionsInfo`]/[`Versions`]
//! (persisted by `ManifestStore` for ETag validation).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::manifest::VersionsRef;

/// Lightweight versions info, persisted by `ManifestStore` for ETag validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionsInfo {
    pub versions: Versions,
    pub etag: Option<String>,
    pub last_updated: u64,
    /// Lazily parsed + descending-sorted `version_list` — same per-package
    /// memoization as `FullManifest::parsed_versions`, for the versions-list
    /// resolution path.
    #[serde(skip)]
    pub parsed_versions: std::sync::OnceLock<Vec<deno_semver::Version>>,
}

impl VersionsInfo {
    /// Lazily parsed + descending-sorted version list.
    pub fn sorted_parsed_versions(&self) -> &[deno_semver::Version] {
        self.parsed_versions.get_or_init(|| {
            crate::model::manifest::sort_parsed_versions(&self.versions.version_list)
        })
    }
}

impl<'a> From<&'a VersionsInfo> for VersionsRef<'a> {
    fn from(info: &'a VersionsInfo) -> Self {
        VersionsRef::from(&info.versions)
    }
}

/// Version list and dist-tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versions {
    pub version_list: Vec<String>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
}

impl<'a> From<&'a Versions> for VersionsRef<'a> {
    fn from(v: &'a Versions) -> Self {
        Self {
            versions: &v.version_list,
            dist_tags: &v.dist_tags,
        }
    }
}
