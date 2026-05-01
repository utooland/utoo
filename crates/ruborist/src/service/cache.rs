//! In-memory manifest cache for dependency resolution.
//!
//! ruborist itself only owns the memory tier; persistent storage (disk, remote
//! KV, …) is delegated to a [`super::store::ManifestStore`] supplied by the
//! host. The project-level cache ([`ProjectCacheData`]) is also pure data —
//! callers load/save it themselves and pass it through `BuildDepsOptions` /
//! `BuildDepsOutput`.
//!
//! # Memory Layout
//!
//! ```text
//!  MemoryCache ─ Clone ─► (cheap: Arc ref-count)
//!  │
//!  └──► Arc<MemoryCacheInner>              single allocation
//!       ├── DashMap<Arc<FullManifest>>      sharded, lock-free reads
//!       ├── DashMap<Arc<VersionsInfo>>
//!       └── DashMap<Arc<CoreVersionManifest>>
//!                           │
//!                           ▼
//!                    All values Arc-wrapped → get/set is O(1) ref-count,
//!                    no full clone of the (large) manifest payload.
//!
//!  Global singleton: GLOBAL_MEMORY_CACHE (LazyLock)
//!  └── all UnifiedRegistry instances share the same cache
//! ```
//!
//! # Lookup Flow
//!
//! ```text
//!  resolve(name, spec)
//!    │
//!    ├─ 1. Memory hit?       ──yes──► Arc<CoreVersionManifest> clone → done
//!    ├─ 2. ManifestStore hit? ──yes──► populate memory → done
//!    └─ 3. Network            ──────► fetch JSON → store memory + fire-and-forget
//!                                       ManifestStore::store_*
//! ```

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::model::manifest::{CoreVersionManifest, FullManifest, VersionsRef};

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

// ============================================================================
// Memory cache (lock-free reads via DashMap)
// ============================================================================

/// Thread-safe in-memory manifest cache. Uses sharded `DashMap`s so concurrent
/// reads are lock-free across shards and writes only contend within a single
/// shard; values are stored as `Arc<…>` so reads return cheap ref-count clones
/// instead of cloning the full (potentially large) manifest payload.
#[derive(Clone)]
pub struct MemoryCache(Arc<MemoryCacheInner>);

struct MemoryCacheInner {
    full_manifests: DashMap<String, Arc<FullManifest>>,
    versions_info: DashMap<String, Arc<VersionsInfo>>,
    version_manifests: DashMap<String, Arc<CoreVersionManifest>>,
}

/// Global singleton. All `UnifiedRegistry` instances share the same cache.
static GLOBAL_MEMORY_CACHE: LazyLock<MemoryCache> = LazyLock::new(|| {
    MemoryCache(Arc::new(MemoryCacheInner {
        full_manifests: DashMap::new(),
        versions_info: DashMap::new(),
        version_manifests: DashMap::new(),
    }))
});

impl Default for MemoryCache {
    fn default() -> Self {
        GLOBAL_MEMORY_CACHE.clone()
    }
}

impl MemoryCache {
    pub fn get_full_manifest(&self, name: &str) -> Option<Arc<FullManifest>> {
        self.0.full_manifests.get(name).map(|v| v.clone())
    }

    pub fn set_full_manifest(&self, name: String, manifest: Arc<FullManifest>) {
        self.0.full_manifests.insert(name, manifest);
    }

    pub fn get_versions(&self, name: &str) -> Option<Arc<VersionsInfo>> {
        self.0.versions_info.get(name).map(|v| v.clone())
    }

    pub fn set_versions(&self, name: String, info: Arc<VersionsInfo>) {
        self.0.versions_info.insert(name, info);
    }

    pub fn get_version_manifest(
        &self,
        name: &str,
        version: &str,
    ) -> Option<Arc<CoreVersionManifest>> {
        let key = format!("{name}@{version}");
        self.0.version_manifests.get(&key).map(|v| v.clone())
    }

    pub fn set_version_manifest(
        &self,
        name: String,
        version: String,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let key = format!("{name}@{version}");
        self.0.version_manifests.insert(key, manifest);
    }

    pub fn full_manifest_count(&self) -> usize {
        self.0.full_manifests.len()
    }

    pub fn versions_count(&self) -> usize {
        self.0.versions_info.len()
    }

    pub fn version_manifest_count(&self) -> usize {
        self.0.version_manifests.len()
    }

    /// Export all version manifests for persistence into a project cache.
    pub fn export_version_manifests(&self) -> Vec<(String, Arc<CoreVersionManifest>)> {
        self.0
            .version_manifests
            .iter()
            .map(|kv| (kv.key().clone(), kv.value().clone()))
            .collect()
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            full_manifest_count: self.full_manifest_count(),
            versions_count: self.versions_count(),
            version_manifest_count: self.version_manifest_count(),
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub full_manifest_count: usize,
    pub versions_count: usize,
    pub version_manifest_count: usize,
}

/// Alias kept so call sites that pre-date the disk-cache split can continue
/// to spell the in-memory cache as `PackageCache` without churn.
pub type PackageCache = MemoryCache;

// ============================================================================
// Project-level cache (per-project resolved packages)
// ============================================================================

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_cache_full_manifest() {
        let cache = MemoryCache::default();

        let manifest = FullManifest {
            name: "test".to_string(),
            ..Default::default()
        };

        cache.set_full_manifest("test".to_string(), Arc::new(manifest));

        let retrieved = cache.get_full_manifest("test").unwrap();
        assert_eq!(retrieved.name, "test");
        assert!(cache.full_manifest_count() >= 1);
    }

    #[test]
    fn test_memory_cache_versions() {
        let cache = MemoryCache::default();

        let info = VersionsInfo {
            versions: Versions {
                version_list: vec!["1.0.0".to_string()],
                dist_tags: HashMap::new(),
            },
            etag: Some("abc".to_string()),
            last_updated: 12345,
        };

        cache.set_versions("test".to_string(), Arc::new(info));

        let retrieved = cache.get_versions("test").unwrap();
        assert_eq!(retrieved.versions.version_list, vec!["1.0.0"]);
        assert!(cache.versions_count() >= 1);
    }

    #[test]
    fn test_memory_cache_version_manifest() {
        let cache = MemoryCache::default();

        let manifest = CoreVersionManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };

        cache.set_version_manifest("test".to_string(), "1.0.0".to_string(), Arc::new(manifest));

        let retrieved = cache.get_version_manifest("test", "1.0.0").unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(retrieved.version, "1.0.0");
        assert!(cache.version_manifest_count() >= 1);
    }
}
