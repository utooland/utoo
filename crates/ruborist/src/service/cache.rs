//! Three-tier manifest cache for dependency resolution.
//!
//! Provides a unified caching layer that works across platforms:
//! - **Memory cache**: Fast in-memory lookup (platform-specific synchronization)
//! - **Disk cache**: Persistent storage (via tokio-fs-ext)
//! - **Project cache**: Per-project resolved packages
//!
//! # Architecture
//!
//! ```text
//!  PackageCache                    (entry point, delegates to tiers)
//!  ├── memory: MemoryCache         tier 1 — in-process, lost on restart
//!  └── cache_dir: Option<PathBuf>  tier 2 — persistent, ~/.utoo/cache/
//!
//!  ProjectCacheData                tier 3 — per-project .utoo-manifest.json
//!  └── cache: HashMap<name, ProjectPackageCache>
//!      ├── specs:     spec → version      ("^2.1" → "2.1.1")
//!      └── manifests: version → CoreVersionManifest  (serde, owned)
//! ```
//!
//! # Memory Layout
//!
//! ```text
//!  MemoryCache ─ Clone ─► (cheap: Arc ref-count)
//!  │
//!  └──► Arc<MemoryCacheInner>              single allocation
//!       ├── RwLock<HashMap<FullManifest>>   full package metadata
//!       ├── RwLock<HashMap<VersionsInfo>>   version lists + dist-tags
//!       └── RwLock<HashMap<Arc<CoreVersionManifest>>>
//!                           │
//!                           ▼
//!                    Arc<CoreVersionManifest>   value-level sharing
//!                    ├── get → Arc clone    O(1) ref-count
//!                    ├── set → Arc clone    O(1) ref-count
//!                    └── used by PackageNode, ResolvedPackage, preload
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
//!    ├─ 1. Memory hit?  ──yes──► Arc<CoreVersionManifest> clone → done
//!    │
//!    ├─ 2. Disk hit?    ──yes──► deserialize → Arc::new() → store memory → done
//!    │
//!    └─ 3. Network      ──────► fetch JSON → Arc::new() → store memory + disk → done
//!
//!  Project cache is loaded at startup (cold) and saved at end (cold).
//!  It pre-populates memory cache to skip network on subsequent runs.
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::manifest::{CoreVersionManifest, FullManifest};

/// Lightweight versions info for disk cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionsInfo {
    pub versions: Versions,
    pub etag: Option<String>,
    pub last_updated: u64, // Unix timestamp
}

/// Version list and dist-tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versions {
    pub version_list: Vec<String>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
}

// ============================================================================
// Memory cache implementation (thread-safe with parking_lot::RwLock)
// ============================================================================

use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;

/// Thread-safe in-memory cache using a single `Arc` wrapper with
/// per-collection `parking_lot::RwLock` for fine-grained concurrency.
#[derive(Clone)]
pub struct MemoryCache(Arc<MemoryCacheInner>);

struct MemoryCacheInner {
    full_manifests: RwLock<HashMap<String, FullManifest>>,
    versions_info: RwLock<HashMap<String, VersionsInfo>>,
    version_manifests: RwLock<HashMap<String, Arc<CoreVersionManifest>>>,
}

/// Global singleton for memory cache.
/// This ensures all UnifiedRegistry instances share the same cache.
static GLOBAL_MEMORY_CACHE: LazyLock<MemoryCache> = LazyLock::new(|| {
    MemoryCache(Arc::new(MemoryCacheInner {
        full_manifests: RwLock::new(HashMap::new()),
        versions_info: RwLock::new(HashMap::new()),
        version_manifests: RwLock::new(HashMap::new()),
    }))
});

impl Default for MemoryCache {
    fn default() -> Self {
        GLOBAL_MEMORY_CACHE.clone()
    }
}

impl MemoryCache {
    /// Get the global memory cache singleton.
    pub fn global() -> Self {
        GLOBAL_MEMORY_CACHE.clone()
    }

    // Full manifests
    pub fn get_full_manifest(&self, name: &str) -> Option<FullManifest> {
        self.0.full_manifests.read().get(name).cloned()
    }

    pub fn set_full_manifest(&self, name: String, manifest: FullManifest) {
        self.0.full_manifests.write().insert(name, manifest);
    }

    // Versions info
    pub fn get_versions(&self, name: &str) -> Option<VersionsInfo> {
        self.0.versions_info.read().get(name).cloned()
    }

    pub fn set_versions(&self, name: String, info: VersionsInfo) {
        self.0.versions_info.write().insert(name, info);
    }

    // Version manifests (Arc-wrapped for cheap cloning)
    pub fn get_version_manifest(
        &self,
        name: &str,
        version: &str,
    ) -> Option<Arc<CoreVersionManifest>> {
        let key = format!("{name}@{version}");
        self.0.version_manifests.read().get(&key).cloned()
    }

    pub fn set_version_manifest(
        &self,
        name: String,
        version: String,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let key = format!("{name}@{version}");
        self.0.version_manifests.write().insert(key, manifest);
    }

    // Stats
    pub fn full_manifest_count(&self) -> usize {
        self.0.full_manifests.read().len()
    }

    pub fn versions_count(&self) -> usize {
        self.0.versions_info.read().len()
    }

    pub fn version_manifest_count(&self) -> usize {
        self.0.version_manifests.read().len()
    }

    /// Export all version manifests for persistence.
    pub fn export_version_manifests(&self) -> Vec<(String, Arc<CoreVersionManifest>)> {
        self.0
            .version_manifests
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

// ============================================================================
// Disk cache paths
// ============================================================================

/// Get the path for package versions cache file.
///
/// Structure: `{cache_dir}/{package_name}/versions.json`
pub fn get_versions_cache_path(cache_dir: &Path, package_name: &str) -> PathBuf {
    cache_dir.join(package_name).join("versions.json")
}

/// Get the path for package manifest cache file.
///
/// Structure: `{cache_dir}/{package_name}/manifests/{version}.json`
pub fn get_manifest_cache_path(cache_dir: &Path, package_name: &str, version: &str) -> PathBuf {
    cache_dir
        .join(package_name)
        .join("manifests")
        .join(format!("{version}.json"))
}

// ============================================================================
// Unified PackageCache
// ============================================================================

/// Three-tier package cache.
///
/// Provides unified access to memory, disk, and project caches.
#[derive(Clone, Default)]
pub struct PackageCache {
    /// In-memory cache (platform-specific implementation)
    memory: MemoryCache,
    /// Disk cache directory (None = no disk cache)
    cache_dir: Option<PathBuf>,
    /// Lazily-populated set of package names with existing disk cache.
    ///
    /// Built once per process from a single `read_dir(cache_dir)` +
    /// per-scope `read_dir` for `@scope/pkg` entries. `get_versions_from_disk`
    /// checks this set first and short-circuits to `None` when the package
    /// isn't on disk — avoiding the 2730 per-package `try_exists`
    /// spawn_blocking calls that dominated cold-preload critical path
    /// (avg 16 ms per call × 128 parallel futures = measurable wall drag).
    ///
    /// Mid-run writes via `set_versions_to_disk` are not reflected in this
    /// index — that's intentional. Any package we wrote in the current run
    /// has already been populated into the memory cache, which takes
    /// precedence over disk; the disk path is only consulted for packages
    /// *not yet seen this run* with *existing disk cache from a prior run*.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    disk_index: Arc<tokio::sync::OnceCell<HashSet<String>>>,
}

impl PackageCache {
    /// Create a new package cache.
    pub fn new() -> Self {
        Self {
            memory: MemoryCache::global(),
            cache_dir: None,
            disk_index: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Create a package cache with disk caching enabled.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            memory: MemoryCache::global(),
            cache_dir: Some(cache_dir),
            disk_index: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Return `true` if the package has a disk cache entry (based on the
    /// lazily-populated bulk-readdir index). Returns `false` if no
    /// cache_dir is configured or the index says the package isn't
    /// present. First call per process does a single directory scan.
    #[cfg(not(target_arch = "wasm32"))]
    async fn has_disk_entry(&self, name: &str) -> bool {
        let Some(cache_dir) = self.cache_dir.as_ref() else {
            return false;
        };
        let cache_dir = cache_dir.clone();
        let index = self
            .disk_index
            .get_or_init(|| async move { build_disk_index(&cache_dir).await })
            .await;
        index.contains(name)
    }

    /// Get the cache directory.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    // === Memory cache operations (sync) ===

    /// Get full manifest from memory cache.
    pub fn get_full_manifest(&self, name: &str) -> Option<FullManifest> {
        let result = self.memory.get_full_manifest(name);
        if result.is_some() {
            tracing::debug!("Memory cache hit for full manifest: {name}");
        }
        result
    }

    /// Set full manifest in memory cache.
    pub fn set_full_manifest(&self, name: String, manifest: FullManifest) {
        tracing::debug!("Caching full manifest in memory: {name}");
        self.memory.set_full_manifest(name, manifest);
    }

    /// Get versions info from memory cache.
    pub fn get_versions(&self, name: &str) -> Option<VersionsInfo> {
        let result = self.memory.get_versions(name);
        if result.is_some() {
            tracing::debug!("Memory cache hit for versions: {name}");
        }
        result
    }

    /// Set versions info in memory cache.
    pub fn set_versions(&self, name: String, info: VersionsInfo) {
        tracing::debug!("Caching versions in memory: {name}");
        self.memory.set_versions(name, info);
    }

    /// Get version manifest from memory cache.
    pub fn get_version_manifest(
        &self,
        name: &str,
        version: &str,
    ) -> Option<Arc<CoreVersionManifest>> {
        let result = self.memory.get_version_manifest(name, version);
        if result.is_some() {
            tracing::debug!("Memory cache hit for version manifest: {name}@{version}");
        }
        result
    }

    /// Set version manifest in memory cache.
    pub fn set_version_manifest(
        &self,
        name: String,
        version: String,
        manifest: Arc<CoreVersionManifest>,
    ) {
        tracing::debug!("Caching version manifest in memory: {name}@{version}");
        self.memory.set_version_manifest(name, version, manifest);
    }

    // === Disk cache operations (async, uses tokio-fs-ext) ===

    /// Load versions info from disk cache.
    ///
    /// Short-circuits via the bulk-readdir index when the package isn't
    /// present on disk — avoids the per-package `try_exists` syscall
    /// that was 16 ms avg / 230 ms max on CI's blocking pool under load.
    pub async fn get_versions_from_disk(&self, name: &str) -> Option<VersionsInfo> {
        let cache_dir = self.cache_dir.as_ref()?;
        #[cfg(not(target_arch = "wasm32"))]
        if !self.has_disk_entry(name).await {
            return None;
        }
        let path = get_versions_cache_path(cache_dir, name);

        match super::fs::read_json::<VersionsInfo>(&path).await {
            Ok(info) => {
                tracing::debug!("Disk cache hit for versions: {name}");
                self.memory.set_versions(name.to_string(), info.clone());
                Some(info)
            }
            Err(e) => {
                tracing::debug!("Failed to read versions cache for {name}: {e}");
                None
            }
        }
    }

    /// Save versions info to disk cache (fire-and-forget).
    ///
    /// Spawns the serialisation + write onto the tokio runtime so the caller
    /// returns immediately. The resolve pipeline doesn't depend on the write
    /// completing — disk cache is best-effort for the *next* run. pcap-driven
    /// tuning showed the previous inline `.await` + `serde_json::to_string_pretty`
    /// burned ~1–3 ms per call on the hot path, stalling the main preload
    /// task and causing the 24..62 active-stream dip observed on CI.
    pub fn set_versions_to_disk(&self, name: &str, info: &VersionsInfo) {
        let Some(cache_dir) = self.cache_dir.clone() else {
            return;
        };
        let name = name.to_string();
        let info = info.clone();

        tokio::spawn(async move {
            let path = get_versions_cache_path(&cache_dir, &name);
            if let Some(parent) = path.parent() {
                let _ = tokio_fs_ext::create_dir_all(parent).await;
            }
            match serde_json::to_vec(&info) {
                Ok(bytes) => {
                    if let Err(e) = tokio_fs_ext::write(&path, bytes).await {
                        tracing::debug!("Failed to write versions cache for {name}: {e}");
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to serialize versions for {name}: {e}");
                }
            }
        });
    }

    /// Load version manifest from disk cache.
    pub async fn get_version_manifest_from_disk(
        &self,
        name: &str,
        version: &str,
    ) -> Option<Arc<CoreVersionManifest>> {
        let cache_dir = self.cache_dir.as_ref()?;
        #[cfg(not(target_arch = "wasm32"))]
        if !self.has_disk_entry(name).await {
            return None;
        }
        let path = get_manifest_cache_path(cache_dir, name, version);

        if !super::fs::exists(&path).await {
            return None;
        }

        match super::fs::read_json::<CoreVersionManifest>(&path).await {
            Ok(manifest) => {
                tracing::debug!("Disk cache hit for manifest: {name}@{version}");
                let manifest = Arc::new(manifest);
                // Also cache in memory
                self.memory.set_version_manifest(
                    name.to_string(),
                    version.to_string(),
                    manifest.clone(),
                );
                Some(manifest)
            }
            Err(e) => {
                tracing::debug!("Failed to read manifest cache for {name}@{version}: {e}");
                None
            }
        }
    }

    /// Save version manifest to disk cache (fire-and-forget).
    ///
    /// Takes `Arc<CoreVersionManifest>` so the shared reference moves into the
    /// spawned task without deep-cloning the manifest (they can be 10–50 KB
    /// each). Same rationale as [`Self::set_versions_to_disk`]: the resolve
    /// pipeline was stalling on inline `.await` + pretty JSON serialisation.
    pub fn set_version_manifest_to_disk(
        &self,
        name: &str,
        version: &str,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let Some(cache_dir) = self.cache_dir.clone() else {
            return;
        };
        let name = name.to_string();
        let version = version.to_string();

        tokio::spawn(async move {
            let path = get_manifest_cache_path(&cache_dir, &name, &version);
            if let Some(parent) = path.parent() {
                let _ = tokio_fs_ext::create_dir_all(parent).await;
            }
            match serde_json::to_vec(&*manifest) {
                Ok(bytes) => {
                    if let Err(e) = tokio_fs_ext::write(&path, bytes).await {
                        tracing::debug!("Failed to write manifest cache for {name}@{version}: {e}");
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to serialize manifest for {name}@{version}: {e}");
                }
            }
        });
    }

    /// Export all version manifests for persistence.
    /// Returns iterator of (key, manifest) pairs where key is "name@version".
    pub fn export_version_manifests(&self) -> Vec<(String, Arc<CoreVersionManifest>)> {
        self.memory.export_version_manifests()
    }

    // === Stats ===

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            full_manifest_count: self.memory.full_manifest_count(),
            versions_count: self.memory.versions_count(),
            version_manifest_count: self.memory.version_manifest_count(),
            has_disk_cache: self.cache_dir.is_some(),
        }
    }
}

/// Scan `cache_dir` once to build a set of package names with existing
/// disk cache entries.
///
/// Layout: `{cache_dir}/<name>/versions.json` for bare packages,
/// `{cache_dir}/@scope/<pkg>/versions.json` for scoped packages. One
/// top-level `read_dir` plus one `read_dir` per scope directory is
/// enough to enumerate every possible name. Unreadable entries are
/// silently skipped (cache dir doesn't exist on cold runs → empty
/// index → every lookup short-circuits to `None`).
#[cfg(not(target_arch = "wasm32"))]
async fn build_disk_index(cache_dir: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let Ok(mut entries) = tokio_fs_ext::read_dir(cache_dir).await else {
        return set;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let raw_name = entry.file_name();
        let Some(name) = raw_name.to_str() else {
            continue;
        };
        if let Some(stripped) = name.strip_prefix('@') {
            // Scoped: read one level deeper. The `@` prefix distinguishes
            // scopes from bare package names unambiguously (npm package
            // names cannot start with `@` unless scoped).
            let scope_path = entry.path();
            let Ok(mut sub) = tokio_fs_ext::read_dir(&scope_path).await else {
                continue;
            };
            while let Ok(Some(sub_entry)) = sub.next_entry().await {
                if let Some(pkg) = sub_entry.file_name().to_str() {
                    set.insert(format!("@{stripped}/{pkg}"));
                }
            }
        } else {
            set.insert(name.to_string());
        }
    }
    tracing::debug!(
        "Built disk cache index: {} package(s) at {}",
        set.len(),
        cache_dir.display()
    );
    set
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub full_manifest_count: usize,
    pub versions_count: usize,
    pub version_manifest_count: usize,
    pub has_disk_cache: bool,
}

// ============================================================================
// Project-level cache (per-project resolved packages)
// ============================================================================

/// Project-level cache data structure.
///
/// Stores resolved package information for a specific project.
/// This is typically stored in `.utoo-manifest.json` in the project root.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectCacheData {
    /// Map of package name -> (spec_map, manifest_map)
    /// spec_map: spec -> version
    /// manifest_map: version -> manifest
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

/// Thread-safe project cache for dependency resolution state.
///
/// This cache stores:
/// - spec -> version mappings (which version was resolved for each spec)
/// - version -> manifest mappings (resolved manifest data)
#[derive(Clone, Default)]
pub struct ProjectCache {
    data: Arc<RwLock<ProjectCacheData>>,
}

impl ProjectCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_resolved_version(&self, name: &str, spec: &str) -> Option<String> {
        self.data
            .read()
            .cache
            .get(name)
            .and_then(|pkg| pkg.specs.get(spec))
            .cloned()
    }

    pub fn get_manifest(&self, name: &str, version: &str) -> Option<Arc<CoreVersionManifest>> {
        self.data
            .read()
            .cache
            .get(name)
            .and_then(|pkg| pkg.manifests.get(version))
            .map(|m| Arc::new(m.clone()))
    }

    pub fn set_resolved(
        &self,
        name: &str,
        spec: &str,
        version: &str,
        manifest: &CoreVersionManifest,
    ) {
        let mut data = self.data.write();
        let pkg = data.cache.entry(name.to_string()).or_default();
        pkg.specs.insert(spec.to_string(), version.to_string());
        pkg.manifests.insert(version.to_string(), manifest.clone());
    }

    pub fn export(&self) -> ProjectCacheData {
        self.data.read().clone()
    }

    pub fn import(&self, data: ProjectCacheData) {
        *self.data.write() = data;
    }

    pub fn clear(&self) {
        self.data.write().cache.clear();
    }
}

/// Load project cache from file.
pub async fn load_project_cache(path: &Path) -> Result<ProjectCacheData> {
    if !super::fs::exists(path).await {
        tracing::debug!("Project cache file not found: {}", path.display());
        return Ok(ProjectCacheData::default());
    }

    let data: ProjectCacheData = super::fs::read_json(path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read/parse project cache: {}", e))?;

    tracing::debug!("Loaded project cache from {}", path.display());
    Ok(data)
}

/// Save project cache to file.
pub async fn save_project_cache(path: &Path, data: &ProjectCacheData) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        let _ = tokio_fs_ext::create_dir_all(parent).await;
    }

    let content =
        serde_json::to_string_pretty(data).context("Failed to serialize project cache")?;

    tokio_fs_ext::write(path, content.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to write project cache: {}", e))?;

    tracing::debug!("Saved project cache to {}", path.display());
    Ok(())
}

// ============================================================================
// Legacy alias for backward compatibility
// ============================================================================

/// Alias for backward compatibility.
pub type ManifestCache = MemoryCache;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_cache_full_manifest() {
        let cache = MemoryCache::global();

        let manifest = FullManifest {
            name: "test".to_string(),
            ..Default::default()
        };

        cache.set_full_manifest("test".to_string(), manifest.clone());

        let retrieved = cache.get_full_manifest("test").unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(cache.full_manifest_count(), 1);
    }

    #[test]
    fn test_memory_cache_versions() {
        let cache = MemoryCache::global();

        let info = VersionsInfo {
            versions: Versions {
                version_list: vec!["1.0.0".to_string()],
                dist_tags: HashMap::new(),
            },
            etag: Some("abc".to_string()),
            last_updated: 12345,
        };

        cache.set_versions("test".to_string(), info.clone());

        let retrieved = cache.get_versions("test").unwrap();
        assert_eq!(retrieved.versions.version_list, vec!["1.0.0"]);
        assert_eq!(cache.versions_count(), 1);
    }

    #[test]
    fn test_memory_cache_version_manifest() {
        let cache = MemoryCache::global();

        let manifest = CoreVersionManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };

        cache.set_version_manifest("test".to_string(), "1.0.0".to_string(), Arc::new(manifest));

        let retrieved = cache.get_version_manifest("test", "1.0.0").unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(retrieved.version, "1.0.0");
        assert_eq!(cache.version_manifest_count(), 1);
    }

    #[test]
    fn test_cache_paths() {
        let cache_dir = PathBuf::from("/tmp/cache");

        let versions_path = get_versions_cache_path(&cache_dir, "lodash");
        assert_eq!(
            versions_path,
            PathBuf::from("/tmp/cache/lodash/versions.json")
        );

        let manifest_path = get_manifest_cache_path(&cache_dir, "lodash", "4.17.21");
        assert_eq!(
            manifest_path,
            PathBuf::from("/tmp/cache/lodash/manifests/4.17.21.json")
        );
    }

    #[test]
    fn test_cache_paths_scoped_package() {
        let cache_dir = PathBuf::from("/tmp/cache");

        let versions_path = get_versions_cache_path(&cache_dir, "@types/node");
        assert_eq!(
            versions_path,
            PathBuf::from("/tmp/cache/@types/node/versions.json")
        );

        let manifest_path = get_manifest_cache_path(&cache_dir, "@types/node", "18.0.0");
        assert_eq!(
            manifest_path,
            PathBuf::from("/tmp/cache/@types/node/manifests/18.0.0.json")
        );
    }

    #[test]
    fn test_package_cache_stats() {
        // Note: MemoryCache uses global singleton, so we can't assert initial values are 0
        // Instead, just verify the stats structure works correctly
        let cache = PackageCache::new();
        let stats = cache.stats();

        // Verify stats fields are accessible (values may vary due to global cache)
        let _ = stats.full_manifest_count;
        let _ = stats.versions_count;
        let _ = stats.version_manifest_count;
        assert!(!stats.has_disk_cache);
    }

    #[test]
    fn test_package_cache_with_disk() {
        let cache = PackageCache::with_cache_dir(PathBuf::from("/tmp/cache"));
        let stats = cache.stats();

        assert!(stats.has_disk_cache);
        assert_eq!(cache.cache_dir(), Some(Path::new("/tmp/cache")));
    }
}
