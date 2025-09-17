use anyhow::{Context, Result};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

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
    info: Arc<VersionsInfo>,
}

impl CachedVersionsInfo {
    fn new(info: VersionsInfo) -> Self {
        Self {
            info: Arc::new(info),
        }
    }
}

pub static PACKAGE_CACHE: Lazy<PackageCache> = Lazy::new(PackageCache::new);

#[derive(Debug)]
pub struct PackageCache {
    cache: Arc<RwLock<CacheMap>>,
    // Per-package cache shards: package_name -> versions info cache
    package_shards: Arc<DashMap<String, Arc<RwLock<Option<CachedVersionsInfo>>>>>,
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
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            package_shards: Arc::new(DashMap::new()),
        }
    }

    /// Load versions info from per-package versions file
    async fn load_versions_from_shard(&self, name: &str) -> Option<VersionsInfo> {
        let versions_file = get_package_versions_cache_file(name);

        if !tokio::fs::try_exists(&versions_file).await.unwrap_or(false) {
            log_verbose(&format!("No versions file found for package {name}"));
            return None;
        }

        match tokio::fs::read_to_string(&versions_file).await {
            Ok(content) => {
                match serde_json::from_str::<VersionsInfo>(&content) {
                    Ok(versions_info) => {
                        log_verbose(&format!("Loaded versions for {name} from shard"));
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

    pub async fn get_full_manifests(&self, name: &str) -> Option<PackageInfo> {
        // Get or create the shard for this specific package
        let shard = self
            .package_shards
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(None)))
            .clone();

        // Check memory cache first
        if let Ok(cached) = shard.try_read() {
            if let Some(ref cached_info) = *cached {
                log_verbose(&format!("Memory cache hit for {name}"));
                let versions_info = Arc::clone(&cached_info.info);
                drop(cached);
                return Some(self.reconstruct_package_info(&versions_info, name).await);
            }
        }

        // Load from disk if not in memory
        log_verbose(&format!("Loading {name} from disk"));
        let versions_info = self.load_versions_from_shard(name).await;

        match versions_info {
            Some(info) => {
                log_verbose(&format!("Package versions loaded for {name}"));
                let info_arc = Arc::new(info);

                // Update memory cache
                if let Ok(mut cached) = shard.try_write() {
                    *cached = Some(CachedVersionsInfo::new((*info_arc).clone()));
                    log_verbose(&format!("Cache updated for {name}"));
                }

                Some(self.reconstruct_package_info(&info_arc, name).await)
            }
            None => {
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
                "Reconstructing package info for {name} with {} versions",
                version_list.len()
            ));
            let mut versions_obj = serde_json::json!({});

            // Load cached manifests from disk - simplified without IO semaphore
            for version_name in version_list {
                if let Some(version_str) = version_name.as_str() {
                    let manifest_file = get_package_manifest_cache_file(name, version_str);
                    // Only load if file exists and is readable
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

            // Create minimal placeholder for versions not found in cache
            if versions_obj.as_object().unwrap().is_empty() {
                for version_name in version_list {
                    if let Some(version_str) = version_name.as_str() {
                        // Create minimal placeholder for semver resolution
                        versions_obj[version_str] = serde_json::json!({
                            "name": name,
                            "version": version_str,
                            "_placeholder": true
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

        // Update package-specific memory cache immediately
        {
            let mut cached = shard.write().await;
            *cached = Some(CachedVersionsInfo::new(versions_info.clone()));
        }
        log_verbose(&format!("Updated memory cache for {name}"));

        // Write to sharded storage asynchronously (simplified)
        let versions_info_clone = versions_info.clone();
        let resolved_version_clone = resolved_version.map(|(v, d)| (v.to_string(), d.clone()));
        let name_clone = name.to_string();

        tokio::spawn(async move {
            if let Err(e) = Self::write_package_to_shards(
                &name_clone,
                &versions_info_clone,
                resolved_version_clone.as_ref(),
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

    /// Write package data to sharded storage (simplified without IO semaphore)
    async fn write_package_to_shards(
        name: &str,
        versions_info: &VersionsInfo,
        resolved_version: Option<&(String, Value)>,
    ) -> Result<()> {
        // Ensure cache directory exists
        let cache_dir = get_package_cache_dir(name);
        if let Err(e) = tokio::fs::create_dir_all(&cache_dir).await {
            return Err(anyhow::anyhow!("Failed to create cache directory: {e}"));
        }

        // Write versions file
        let versions_file = get_package_versions_cache_file(name);
        let versions_content = serde_json::to_string_pretty(versions_info)?;
        tokio::fs::write(&versions_file, versions_content).await?;

        // Write resolved version manifest if provided
        if let Some((version, manifest)) = resolved_version {
            let version_manifest = VersionManifest {
                manifest: manifest.clone(),
                last_updated: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            let manifest_file = get_package_manifest_cache_file(name, version);
            let manifest_content = serde_json::to_string_pretty(&version_manifest)?;
            tokio::fs::write(&manifest_file, manifest_content).await?;
        }

        Ok(())
    }

    /// Cache version manifest to disk
    pub async fn cache_version_manifest(&self, name: &str, version: &str, manifest: &Value) {
        let version_manifest = VersionManifest {
            manifest: manifest.clone(),
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let manifest_file = get_package_manifest_cache_file(name, version);

        // Ensure directory exists
        if let Some(parent) = manifest_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        if let Ok(content) = serde_json::to_string_pretty(&version_manifest) {
            if let Err(e) = tokio::fs::write(&manifest_file, content).await {
                log_verbose(&format!("Failed to cache manifest for {name}@{version}: {e}"));
            } else {
                log_verbose(&format!("Cached manifest for {name}@{version}"));
            }
        }
    }

}

// Utility functions for project-level cache management (not global package cache)
pub async fn load_cache(path: &Path) -> Result<()> {
    if !tokio::fs::try_exists(path)
        .await
        .context("Failed to check cache file existence")?
    {
        log_verbose(&format!("Project cache file not found: {}", path.display()));
        return Ok(());
    }

    let cache_str = tokio::fs::read_to_string(path)
        .await
        .context("Failed to read cache file")?;
    let cache_data: CacheData = serde_json::from_str(&cache_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse cache data: {}", e))?;

    PACKAGE_CACHE.import_data(cache_data).await;
    log_verbose(&format!("Project cache loaded from {}", path.display()));
    Ok(())
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
    log_verbose(&format!("Project cache stored to {}", path.display()));
    Ok(())
}

pub async fn flush_cache_to_disk() -> Result<()> {
    PACKAGE_CACHE.flush_to_disk().await
}
