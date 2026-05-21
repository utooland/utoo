//! Manifest cache data structures for dependency resolution.
//!
//! The demand BFS loop owns the in-memory manifest maps for one resolution run.
//! This module only carries serializable data shared between the loop,
//! provider jobs, and host persistence.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::manifest::{CoreVersionManifest, VersionsRef};

/// Lightweight versions info, persisted by `ManifestStore` for ETag validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionsInfo {
    pub versions: Versions,
    pub etag: Option<String>,
    pub last_updated: u64,
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

/// Project-level cache data.
///
/// Stores resolved package information for a specific project. Hosts persist
/// this (typically as `node_modules/.utoo-manifest.json`) and pass it back via
/// `BuildDepsOptions::warm_project_cache`.
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
