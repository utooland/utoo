//! Manifest cache data types for dependency resolution.
//!
//! ruborist owns no resolution-time cache state — the demand resolver holds the
//! per-run manifest store, and persistent storage (disk, remote KV, …) is
//! delegated to a [`super::store::ManifestStore`] supplied by the host. This
//! module holds the shared, pure-data types: [`VersionsInfo`]/[`Versions`]
//! (persisted by `ManifestStore` for ETag validation) and the project-level
//! cache ([`ProjectCacheData`]) that hosts load/save and pass through
//! `BuildDepsOptions` / `BuildDepsOutput`.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::model::manifest::{CoreVersionManifest, VersionsRef};

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

// ============================================================================
// Project-level cache (per-project resolved packages)
// ============================================================================

/// Project-level cache data.
///
/// Stores resolved package information for a specific project. Hosts persist
/// this (typically as `node_modules/.utoo-manifest.json`) and pass it back via
/// `BuildDepsOptions::project_cache`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectCacheData {
    /// package name -> per-package cache
    #[serde(default)]
    pub cache: HashMap<String, ProjectPackageCache>,
}

/// Per-package cache in project cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectPackageCache {
    /// spec -> resolved version
    #[serde(default)]
    pub specs: HashMap<String, String>,
    /// version -> manifest
    #[serde(default)]
    pub manifests: HashMap<String, CoreVersionManifest>,
}

impl ProjectCacheData {
    /// Flatten into neutral `(name, spec, manifest)` tuples for seeding an
    /// in-memory resolver store. The store stays unaware of this on-disk shape.
    pub(crate) fn resolved_manifests(&self) -> Vec<(String, String, Arc<CoreVersionManifest>)> {
        let mut out = Vec::new();
        for (name, pkg) in &self.cache {
            // Build one Arc per resolved version, then share it across every
            // spec pointing at that version — sibling ranges commonly collapse
            // to the same version, so this avoids cloning the manifest per spec.
            let version_arcs: HashMap<&String, Arc<CoreVersionManifest>> = pkg
                .manifests
                .iter()
                .map(|(version, manifest)| (version, Arc::new(manifest.clone())))
                .collect();
            for (spec, version) in &pkg.specs {
                if let Some(arc) = version_arcs.get(version) {
                    out.push((name.clone(), spec.clone(), Arc::clone(arc)));
                }
            }
        }
        out
    }

    /// Rebuild from the resolver's neutral `(name, spec, manifest)` tuples,
    /// indexing each manifest under both its spec and its resolved version.
    pub(crate) fn from_resolved(entries: Vec<(String, String, Arc<CoreVersionManifest>)>) -> Self {
        let mut data = Self::default();
        for (name, spec, manifest) in entries {
            let version = manifest.version.clone();
            let pkg = data.cache.entry(name).or_default();
            pkg.specs.insert(spec, version.clone());
            pkg.manifests
                .entry(version)
                .or_insert_with(|| (*manifest).clone());
        }
        data
    }
}
