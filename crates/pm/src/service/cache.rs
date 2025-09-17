use anyhow::{Context, Result};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::sync::Semaphore;

use crate::util::cache::{
    get_package_cache_dir, get_package_manifest_cache_file, get_package_versions_cache_file,
};
use crate::util::logger::log_verbose;

type VersionMap = HashMap<String, Value>;
type SpecMap = HashMap<String, String>; // spec -> version
type CacheMap = HashMap<String, (SpecMap, VersionMap)>; // name -> (specs, versions)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub data: Value,
    pub etag: Option<String>,
    pub last_updated: u64, // Unix timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionsInfo {
    pub versions: Value, // versions object from npm registry
    pub etag: Option<String>,
    pub last_updated: u64, // Unix timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub manifest: Value,   // specific version manifest data
    pub last_updated: u64, // Unix timestamp
}

#[derive(Debug, Clone)]
struct CachedVersionsInfo {
    info: Arc<VersionsInfo>, // Use Arc to reduce clone overhead in multi-threaded scenarios
    loaded_at: SystemTime,
}

impl CachedVersionsInfo {
    fn new(info: VersionsInfo) -> Self {
        Self {
            info: Arc::new(info),
            loaded_at: SystemTime::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.loaded_at.elapsed().unwrap_or_default() > ttl
    }
}

pub static PACKAGE_CACHE: Lazy<PackageCache> = Lazy::new(PackageCache::new);

#[derive(Debug)]
pub struct PackageCache {
    cache: Arc<RwLock<CacheMap>>,
    // Per-package cache shards: package_name -> versions info cache
    package_shards: Arc<DashMap<String, Arc<RwLock<Option<CachedVersionsInfo>>>>>,
    // IO concurrency limiter - limit concurrent disk reads to avoid overwhelming the filesystem
    io_semaphore: Arc<Semaphore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    cache: CacheMap,
}

impl Default for PackageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageCache {
    pub fn new() -> Self {
        let instance = Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            package_shards: Arc::new(DashMap::new()),
            // Limit concurrent disk I/O to 20 operations to avoid overwhelming the filesystem
            io_semaphore: Arc::new(Semaphore::new(20)),
        };

        // Start background cleanup task for package shards
        let shards_clone = Arc::clone(&instance.package_shards);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes
            loop {
                interval.tick().await;
                Self::cleanup_package_shards(&shards_clone).await;
            }
        });

        instance
    }

    // Memory cache TTL - 5 minutes to balance performance and memory usage
    const MEMORY_CACHE_TTL: Duration = Duration::from_secs(300);

    /// Load versions info from per-package versions file
    async fn load_versions_from_shard(&self, name: &str) -> Option<VersionsInfo> {
        let versions_file = get_package_versions_cache_file(name);

        if !tokio::fs::try_exists(&versions_file).await.unwrap_or(false) {
            log_verbose(&format!("No versions file found for package {name}"));
            return None;
        }

        // Acquire semaphore permit to limit concurrent disk I/O
        let _permit = self.io_semaphore.acquire().await.ok()?;

        let start = Instant::now();
        match tokio::fs::read_to_string(&versions_file).await {
            Ok(content) => {
                log_verbose(&format!(
                    "Read versions file for {name} in {:?}",
                    start.elapsed()
                ));

                // Release permit before JSON parsing (CPU-intensive work)
                drop(_permit);

                match serde_json::from_str::<VersionsInfo>(&content) {
                    Ok(versions_info) => {
                        log_verbose(&format!(
                            "Loaded versions for {name} from shard in {:?}",
                            start.elapsed()
                        ));
                        Some(versions_info)
                    }
                    Err(e) => {
                        log_verbose(&format!("Failed to parse versions file for {name}: {e}"));
                        None
                    }
                }
            }
            Err(e) => {
                log_verbose(&format!("Failed to read versions file for {name}: {e}"));
                None
            }
        }
    }

    /// Manually flush cache to disk (for graceful shutdown)
    /// Note: Per-package manifests are written asynchronously, so no explicit flushing needed
    pub async fn flush_to_disk(&self) -> Result<()> {
        log_verbose("Cache flush: Per-package manifests are written asynchronously");
        Ok(())
    }

    pub async fn export_data(&self) -> CacheData {
        let cache = self.cache.read().await;
        CacheData {
            cache: cache.clone(),
        }
    }

    pub async fn import_data(&self, data: CacheData) {
        let mut cache = self.cache.write().await;
        *cache = data.cache;
    }

    pub async fn get_manifest(&self, name: &str, _spec: &str, version: &str) -> Option<Value> {
        let cache = self.cache.read().await;
        cache
            .get(name)
            .and_then(|(_, versions)| versions.get(version))
            .cloned()
    }

    pub async fn set_manifest(&self, name: &str, spec: &str, version: &str, manifest: Value) {
        let mut cache = self.cache.write().await;
        let (specs, versions) = cache
            .entry(name.to_string())
            .or_insert_with(|| (HashMap::new(), HashMap::new()));

        specs.insert(spec.to_string(), version.to_string());
        versions.insert(version.to_string(), manifest);
    }

    pub async fn get_version(&self, name: &str, spec: &str) -> Option<String> {
        let cache = self.cache.read().await;
        cache
            .get(name)
            .and_then(|(specs, _)| specs.get(spec))
            .cloned()
    }

    pub async fn get_package_info(&self, name: &str) -> Option<PackageInfo> {
        // Get or create the shard for this specific package
        let shard = self
            .package_shards
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(None)))
            .clone();

        // L1 Cache: Fast read-only check (non-blocking)
        if let Ok(cached) = shard.try_read() {
            if let Some(ref cached_info) = *cached {
                if !cached_info.is_expired(Self::MEMORY_CACHE_TTL) {
                    log_verbose(&format!("Memory cache hit for {name}"));
                    // Arc clone is very cheap - just incrementing reference count
                    let versions_info = Arc::clone(&cached_info.info);
                    // Release read lock before expensive reconstruction
                    drop(cached);
                    return Some(self.reconstruct_package_info(&versions_info, name).await);
                } else {
                    log_verbose(&format!("Memory cache expired for {name}"));
                    // Continue to reload from disk
                }
            }
        } else {
            log_verbose(&format!(
                "🔍 Could not acquire read lock for {name}, loading from disk"
            ));
        }

        // L2 Cache: Load from disk without blocking locks (lockless approach)
        log_verbose(&format!("🔍 Loading {name} from disk (lockless)"));
        let load_start = Instant::now();
        let versions_info = self.load_versions_from_shard(name).await;

        match versions_info {
            Some(info) => {
                log_verbose(&format!(
                    "Package versions loaded {name}, took {:?}",
                    load_start.elapsed()
                ));
                let info_arc = Arc::new(info);

                // Try to update cache, but don't block if we can't
                if let Ok(mut cached) = shard.try_write() {
                    *cached = Some(CachedVersionsInfo {
                        info: Arc::clone(&info_arc),
                        loaded_at: SystemTime::now(),
                    });
                    log_verbose(&format!("🔍 Cache updated for {name} (lockless)"));
                } else {
                    log_verbose(&format!(
                        "🔍 Could not update cache for {name}, continuing anyway"
                    ));
                }

                log_verbose(&format!(
                    "Package versions hit for {name}, loaded to memory"
                ));
                Some(self.reconstruct_package_info(&info_arc, name).await)
            }
            None => {
                // Try to clear cache, but don't block
                if let Ok(mut cached) = shard.try_write() {
                    *cached = None;
                    log_verbose(&format!("🔍 Cache cleared for {name} (lockless)"));
                }
                log_verbose(&format!("No cache found for {name}"));
                None
            }
        }
    }

    /// Reconstruct full PackageInfo from VersionsInfo for backward compatibility
    async fn reconstruct_package_info(
        &self,
        versions_info: &Arc<VersionsInfo>,
        name: &str,
    ) -> PackageInfo {
        // The versions_info now only contains minimal data (time, version list, dist-tags)
        // For backward compatibility, we create a minimal structure that works with existing code
        let mut full_data = versions_info.versions.clone();

        // Extract version list from nested structure: {"versions": {"versions": [...], "dist-tags": {...}}}
        let version_list = full_data
            .get("versions")
            .and_then(|v| v.get("versions"))
            .and_then(|v| v.as_array());

        if let Some(version_list) = version_list {
            log_verbose(&format!(
                "🔍 Reconstructing package info for {name} with {} versions",
                version_list.len()
            ));
            let mut versions_obj = serde_json::json!({});

            // Only load cached manifests (don't try to load all versions from disk)
            // Acquire semaphore permit to limit concurrent file I/O operations
            let available_permits = self.io_semaphore.available_permits();
            log_verbose(&format!(
                "🔍 Acquiring I/O semaphore for {name} reconstruction (available: {available_permits})"
            ));
            let _permit = match self.io_semaphore.acquire().await {
                Ok(permit) => {
                    log_verbose(&format!(
                        "🔍 I/O semaphore acquired for {name} reconstruction (remaining: {})",
                        self.io_semaphore.available_permits()
                    ));
                    Some(permit)
                }
                Err(_) => {
                    log_verbose(&format!(
                        "Failed to acquire I/O semaphore for reconstruct_package_info {name}, skipping manifest loading"
                    ));
                    None
                }
            };

            // Only perform file I/O if we successfully acquired the semaphore permit
            if _permit.is_some() {
                for version_name in version_list {
                    if let Some(version_str) = version_name.as_str() {
                        let manifest_file = get_package_manifest_cache_file(name, version_str);
                        // Only load if file exists and is readable (avoid I/O for non-cached versions)
                        if let Ok(true) = tokio::fs::try_exists(&manifest_file).await
                            && let Ok(content) = tokio::fs::read_to_string(&manifest_file).await
                            && let Ok(version_manifest) =
                                serde_json::from_str::<VersionManifest>(&content)
                        {
                            versions_obj[version_str] = version_manifest.manifest;
                            log_verbose(&format!(
                                "Loaded cached manifest for {name}@{version_str}"
                            ));
                        }
                    }
                }
            } else {
                log_verbose(&format!(
                    "Skipping manifest loading for {name} due to semaphore acquisition failure"
                ));
            }

            // Always create version object structure for backward compatibility
            // If no cached manifests, create minimal placeholder entries for semver resolution
            if versions_obj.as_object().unwrap().is_empty() {
                for version_name in version_list {
                    if let Some(version_str) = version_name.as_str() {
                        // Create minimal placeholder for semver resolution
                        versions_obj[version_str] = serde_json::json!({
                            "name": name,
                            "version": version_str,
                            "_placeholder": true  // Mark as placeholder
                        });
                    }
                }
                log_verbose(&format!(
                    "Created placeholder versions for {name} (no cached manifests)"
                ));
            } else {
                log_verbose(&format!(
                    "Reconstructed package info with {} cached manifests for {name}",
                    versions_obj.as_object().unwrap().len()
                ));
            }

            // Explicitly drop permit to release semaphore
            drop(_permit);
            log_verbose(&format!(
                "🔍 Finished reconstructing package info for {name}, semaphore permit released"
            ));

            // Replace version array with version object and ensure dist-tags are at top level
            full_data["versions"] = versions_obj;

            // Also move dist-tags to top level for backward compatibility
            if let Some(dist_tags) = full_data.get("versions").and_then(|v| v.get("dist-tags")) {
                full_data["dist-tags"] = dist_tags.clone();
            }

            // Move other metadata to top level as well
            if let Some(name_val) = full_data.get("versions").and_then(|v| v.get("name")) {
                full_data["name"] = name_val.clone();
            }
            if let Some(time_val) = full_data.get("versions").and_then(|v| v.get("time")) {
                full_data["time"] = time_val.clone();
            }
        }

        PackageInfo {
            data: full_data,
            etag: versions_info.etag.clone(),
            last_updated: versions_info.last_updated,
        }
    }

    pub async fn set_package_info(&self, name: &str, data: Value, etag: Option<String>) {
        self.set_package_info_with_version(name, data, etag, None)
            .await;
    }

    /// Set package info and optionally cache a specific version manifest
    pub async fn set_package_info_with_version(
        &self,
        name: &str,
        data: Value,
        etag: Option<String>,
        resolved_version: Option<(&str, &Value)>,
    ) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Extract only versions and time data to reduce versions.json size
        let mut versions_data = serde_json::json!({});

        // Keep only essential version metadata
        if let Some(time) = data.get("time") {
            versions_data["time"] = time.clone();
        }
        if let Some(versions) = data.get("versions")
            && let Some(versions_obj) = versions.as_object()
        {
            versions_data["versions"] = serde_json::json!(versions_obj.keys().collect::<Vec<_>>());
        }
        if let Some(dist_tags) = data.get("dist-tags") {
            versions_data["dist-tags"] = dist_tags.clone();
        }

        // Keep metadata for compatibility but minimize size
        if let Some(name) = data.get("name") {
            versions_data["name"] = name.clone();
        }

        let versions_info = VersionsInfo {
            versions: versions_data, // Only time, version list, and dist-tags
            etag: etag.clone(),
            last_updated: current_time,
        };

        // Get or create the shard for this specific package
        let shard = self
            .package_shards
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(None)))
            .clone();

        // Update package-specific memory cache immediately (L1 cache) with optimized lock usage
        {
            let mut cached = shard.write().await;
            *cached = Some(CachedVersionsInfo::new(versions_info.clone()));
            // Explicitly release write lock quickly to reduce contention
        }
        log_verbose(&format!("Updated memory cache for {name}"));

        // Write to sharded storage asynchronously
        let versions_info_clone = versions_info.clone();
        let resolved_version_clone = resolved_version.map(|(v, d)| (v.to_string(), d.clone()));
        let name_clone = name.to_string();
        let io_semaphore = Arc::clone(&self.io_semaphore);

        tokio::spawn(async move {
            if let Err(e) = Self::write_package_to_shards(
                &name_clone,
                &versions_info_clone,
                resolved_version_clone.as_ref(),
                io_semaphore,
            )
            .await
            {
                log_verbose(&format!(
                    "Failed to write package shards for {name_clone}: {e}"
                ));
            }
        });

        log_verbose(&format!("Scheduled package shards write for {name}"));
    }

    /// Write package info to sharded storage (versions.json + only resolved version manifest)
    async fn write_package_to_shards(
        name: &str,
        versions_info: &VersionsInfo,
        resolved_version: Option<&(String, Value)>,
        io_semaphore: Arc<Semaphore>,
    ) -> Result<()> {
        let start = Instant::now();

        // Ensure package directory exists
        let package_dir = get_package_cache_dir(name);
        if let Err(e) = tokio::fs::create_dir_all(&package_dir).await {
            log_verbose(&format!(
                "Failed to create package directory for {name}: {e}"
            ));
            return Ok(());
        }

        // Write versions.json (lightweight file with only metadata)
        {
            let _permit = io_semaphore
                .acquire()
                .await
                .map_err(|_| anyhow::anyhow!("Failed to acquire I/O semaphore permit"))?;

            let versions_file = get_package_versions_cache_file(name);
            match serde_json::to_string_pretty(versions_info) {
                Ok(json_content) => {
                    if let Err(e) = tokio::fs::write(&versions_file, json_content).await {
                        log_verbose(&format!("Failed to write versions file for {name}: {e}"));
                    } else {
                        log_verbose(&format!("Wrote versions file for {name}"));
                    }
                }
                Err(e) => {
                    log_verbose(&format!("Failed to serialize versions for {name}: {e}"));
                }
            }
        }

        // Only write the resolved version manifest (save disk space)
        if let Some((version, manifest_data)) = resolved_version {
            let manifests_dir = package_dir.join("manifests");
            if let Err(e) = tokio::fs::create_dir_all(&manifests_dir).await {
                log_verbose(&format!(
                    "Failed to create manifests directory for {name}: {e}"
                ));
                return Ok(());
            }

            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let version_manifest = VersionManifest {
                manifest: manifest_data.clone(),
                last_updated: current_time,
            };

            let _permit = io_semaphore
                .acquire()
                .await
                .map_err(|_| anyhow::anyhow!("Failed to acquire I/O semaphore permit"))?;

            let manifest_file = get_package_manifest_cache_file(name, version);
            match serde_json::to_string_pretty(&version_manifest) {
                Ok(json_content) => {
                    if let Err(e) = tokio::fs::write(&manifest_file, json_content).await {
                        log_verbose(&format!(
                            "Failed to write manifest for {name}@{version}: {e}"
                        ));
                    } else {
                        log_verbose(&format!(
                            "Wrote resolved manifest for {name}@{version} in {:?}",
                            start.elapsed()
                        ));
                    }
                }
                Err(e) => {
                    log_verbose(&format!(
                        "Failed to serialize manifest for {name}@{version}: {e}"
                    ));
                }
            }
        } else {
            log_verbose(&format!("No resolved version to cache for {name}"));
        }

        Ok(())
    }

    /// Cache a specific version manifest (for on-demand caching)
    pub async fn cache_version_manifest(&self, name: &str, version: &str, manifest: &Value) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let version_manifest = VersionManifest {
            manifest: manifest.clone(),
            last_updated: current_time,
        };

        let name_clone = name.to_string();
        let version_clone = version.to_string();
        let manifest_clone = version_manifest;
        let io_semaphore = Arc::clone(&self.io_semaphore);

        tokio::spawn(async move {
            if let Err(e) = Self::write_single_version_manifest(
                &name_clone,
                &version_clone,
                &manifest_clone,
                io_semaphore,
            )
            .await
            {
                log_verbose(&format!(
                    "Failed to cache manifest for {name_clone}@{version_clone}: {e}"
                ));
            }
        });
    }

    /// Write a single version manifest to disk
    async fn write_single_version_manifest(
        name: &str,
        version: &str,
        version_manifest: &VersionManifest,
        io_semaphore: Arc<Semaphore>,
    ) -> Result<()> {
        let package_dir = get_package_cache_dir(name);
        let manifests_dir = package_dir.join("manifests");

        if let Err(e) = tokio::fs::create_dir_all(&manifests_dir).await {
            log_verbose(&format!(
                "Failed to create manifests directory for {name}: {e}"
            ));
            return Ok(());
        }

        let _permit = io_semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Failed to acquire I/O semaphore permit"))?;

        let manifest_file = get_package_manifest_cache_file(name, version);
        match serde_json::to_string_pretty(version_manifest) {
            Ok(json_content) => {
                if let Err(e) = tokio::fs::write(&manifest_file, json_content).await {
                    log_verbose(&format!(
                        "Failed to write manifest for {name}@{version}: {e}"
                    ));
                } else {
                    log_verbose(&format!("Cached manifest for {name}@{version}"));
                }
            }
            Err(e) => {
                log_verbose(&format!(
                    "Failed to serialize manifest for {name}@{version}: {e}"
                ));
            }
        }

        Ok(())
    }

    /// Cleanup expired entries from package shards
    async fn cleanup_package_shards(
        shards: &DashMap<String, Arc<RwLock<Option<CachedVersionsInfo>>>>,
    ) {
        let before_count = shards.len();
        let mut removed_count = 0;

        // Collect expired shard keys
        let mut expired_keys = Vec::new();
        for entry in shards.iter() {
            let (name, shard) = entry.pair();
            let should_remove = {
                let cached = shard.read().await;
                match &*cached {
                    Some(cached_info) => cached_info.is_expired(Self::MEMORY_CACHE_TTL),
                    None => true, // Remove empty shards too
                }
            };

            if should_remove {
                expired_keys.push(name.clone());
            }
        }

        // Remove expired shards
        for key in expired_keys {
            if shards.remove(&key).is_some() {
                removed_count += 1;
                log_verbose(&format!("Removed expired package shard for {key}"));
            }
        }

        if removed_count > 0 {
            log_verbose(&format!(
                "Package shards cleanup: removed {} entries, {} remaining",
                removed_count,
                before_count - removed_count
            ));
        }
    }
}

pub async fn store_cache(path: &Path) -> Result<()> {
    let cache_data = PACKAGE_CACHE.export_data().await;
    let cache_str =
        serde_json::to_string_pretty(&cache_data).context("Failed to serialize cache data")?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("Failed to create cache directory")?;
    }
    tokio::fs::write(path, cache_str)
        .await
        .context("Failed to write cache file")?;
    log_verbose(&format!("Cache stored to {}", path.display()));
    Ok(())
}

pub async fn load_cache(path: &Path) -> Result<()> {
    if !tokio::fs::try_exists(path)
        .await
        .context("Failed to check cache file existence")?
    {
        log_verbose(&format!("Cache file not found: {}", path.display()));
        return Ok(());
    }

    let cache_str = tokio::fs::read_to_string(path)
        .await
        .context("Failed to read cache file")?;
    let cache_data: CacheData = serde_json::from_str(&cache_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse cache data: {}", e))?;

    PACKAGE_CACHE.import_data(cache_data).await;
    log_verbose(&format!("Cache loaded from {}", path.display()));
    Ok(())
}

/// Flush unified manifest to disk (for graceful shutdown)
pub async fn flush_cache_to_disk() -> Result<()> {
    PACKAGE_CACHE.flush_to_disk().await
}
