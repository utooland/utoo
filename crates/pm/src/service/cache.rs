use anyhow::{Context, Result};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::manifest::{FullManifest, VersionManifest as ModelVersionManifest};
use crate::util::cache::{get_package_manifest_cache_file, get_package_versions_cache_file};
use crate::util::logger::log_verbose;

// Project-level cache for resolved packages
type VersionMap = HashMap<String, Value>;
type SpecMap = HashMap<String, String>; // spec -> version
type CacheMap = HashMap<String, (SpecMap, VersionMap)>; // name -> (specs, versions)

// Lightweight versions info stored in versions.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionsInfo {
    pub versions: Value, // versions object from npm registry
    pub etag: Option<String>,
    pub last_updated: u64, // Unix timestamp
}

// Individual version manifest stored in manifests/version.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub manifest: Value,   // specific version manifest data
    pub last_updated: u64, // Unix timestamp
}

// Memory cache structures for high-performance sync access
#[derive(Debug, Clone)]
struct CachedFullManifest {
    manifest: Arc<FullManifest>,
}

#[derive(Debug, Clone)]
struct CachedVersionsInfo {
    info: Arc<VersionsInfo>,
}

#[derive(Debug, Clone)]
struct CachedVersionManifest {
    manifest: Arc<ModelVersionManifest>,
}

pub static PACKAGE_CACHE: Lazy<PackageCache> = Lazy::new(PackageCache::new);

#[derive(Debug)]
pub struct PackageCache {
    // Project-level cache (async for backward compatibility)
    project_cache: Arc<tokio::sync::RwLock<CacheMap>>,

    // High-performance sync memory caches
    full_manifests: Arc<DashMap<String, CachedFullManifest>>,
    versions_info: Arc<DashMap<String, CachedVersionsInfo>>,
    version_manifests: Arc<DashMap<String, CachedVersionManifest>>, // key: "name@version"
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
            project_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            full_manifests: Arc::new(DashMap::new()),
            versions_info: Arc::new(DashMap::new()),
            version_manifests: Arc::new(DashMap::new()),
        }
    }

    // === Synchronous memory cache operations ===

    /// Get full manifest from memory cache (sync, high performance)
    pub fn get_full_manifest(&self, name: &str) -> Option<FullManifest> {
        self.full_manifests.get(name).map(|cached| {
            log_verbose(&format!("Memory cache hit for full manifest: {name}"));
            (*cached.manifest).clone()
        })
    }

    /// Set full manifest in memory cache (sync)
    pub fn set_full_manifest(&self, name: &str, manifest: &FullManifest) {
        let cached = CachedFullManifest {
            manifest: Arc::new(manifest.clone()),
        };
        self.full_manifests.insert(name.to_string(), cached);
        log_verbose(&format!("Cached full manifest in memory: {name}"));
    }

    /// Get versions info from memory cache (sync)
    pub fn get_versions(&self, name: &str) -> Option<(Option<String>, VersionsInfo)> {
        self.versions_info.get(name).map(|cached| {
            log_verbose(&format!("Memory cache hit for versions: {name}"));
            (cached.info.etag.clone(), (*cached.info).clone())
        })
    }

    /// Set versions info in memory cache (sync)
    pub fn set_versions(&self, name: &str, versions: &VersionsInfo, etag: Option<String>) {
        let mut versions_with_etag = versions.clone();
        versions_with_etag.etag = etag;

        let cached = CachedVersionsInfo {
            info: Arc::new(versions_with_etag),
        };
        self.versions_info.insert(name.to_string(), cached);
        log_verbose(&format!("Cached versions info in memory: {name}"));
    }

    /// Get version manifest from memory cache (sync)
    pub fn get_version_manifest(&self, name: &str, version: &str) -> Option<ModelVersionManifest> {
        let key = format!("{name}@{version}");
        self.version_manifests.get(&key).map(|cached| {
            log_verbose(&format!(
                "Memory cache hit for version manifest: {name}@{version}"
            ));
            (*cached.manifest).clone()
        })
    }

    /// Set version manifest in memory cache (sync)
    pub fn set_version_manifest(&self, name: &str, version: &str, manifest: &ModelVersionManifest) {
        let key = format!("{name}@{version}");
        let cached = CachedVersionManifest {
            manifest: Arc::new(manifest.clone()),
        };
        self.version_manifests.insert(key, cached);
        log_verbose(&format!(
            "Cached version manifest in memory: {name}@{version}"
        ));
    }

    // === Async disk operations ===

    /// Load versions info from disk (returns etag and versions info)
    pub async fn get_versions_from_disk(
        &self,
        name: &str,
    ) -> (Option<String>, Option<VersionsInfo>) {
        let versions_file = get_package_versions_cache_file(name);

        if !tokio::fs::try_exists(&versions_file).await.unwrap_or(false) {
            log_verbose(&format!("No versions file found for package: {name}"));
            return (None, None);
        }

        match tokio::fs::read_to_string(&versions_file).await {
            Ok(content) => match serde_json::from_str::<VersionsInfo>(&content) {
                Ok(versions_info) => {
                    log_verbose(&format!("Loaded versions from disk: {name}"));
                    let etag = versions_info.etag.clone();
                    (etag, Some(versions_info))
                }
                Err(e) => {
                    log_verbose(&format!("Failed to parse versions file for {name}: {e}"));
                    (None, None)
                }
            },
            Err(e) => {
                log_verbose(&format!("Failed to read versions file for {name}: {e}"));
                (None, None)
            }
        }
    }

    /// Write versions info to disk (async)
    pub async fn set_versions_to_disk(&self, name: &str, versions: &VersionsInfo) {
        let versions_file = get_package_versions_cache_file(name);

        // Ensure directory exists
        if let Some(parent) = versions_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        match serde_json::to_string_pretty(versions) {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(&versions_file, content).await {
                    log_verbose(&format!("Failed to write versions file for {name}: {e}"));
                } else {
                    log_verbose(&format!("Wrote versions to disk: {name}"));
                }
            }
            Err(e) => {
                log_verbose(&format!("Failed to serialize versions for {name}: {e}"));
            }
        }
    }

    /// Load version manifest from disk
    pub async fn get_version_manifest_from_disk(
        &self,
        name: &str,
        version: &str,
    ) -> Option<ModelVersionManifest> {
        let manifest_file = get_package_manifest_cache_file(name, version);

        if !tokio::fs::try_exists(&manifest_file).await.unwrap_or(false) {
            log_verbose(&format!("No manifest file found for {name}@{version}"));
            return None;
        }

        match tokio::fs::read_to_string(&manifest_file).await {
            Ok(content) => match serde_json::from_str::<VersionManifest>(&content) {
                Ok(cached_manifest) => {
                    log_verbose(&format!("Loaded manifest from disk: {name}@{version}"));
                    // Convert to ModelVersionManifest
                    Some(
                        serde_json::from_value(cached_manifest.manifest).unwrap_or_else(|e| {
                            log_verbose(&format!(
                                "Failed to convert cached manifest for {name}@{version}: {e}"
                            ));
                            // Return a minimal version manifest as fallback
                            ModelVersionManifest {
                                name: name.to_string(),
                                version: version.to_string(),
                                ..Default::default()
                            }
                        }),
                    )
                }
                Err(e) => {
                    log_verbose(&format!(
                        "Failed to parse manifest file for {name}@{version}: {e}"
                    ));
                    None
                }
            },
            Err(e) => {
                log_verbose(&format!(
                    "Failed to read manifest file for {name}@{version}: {e}"
                ));
                None
            }
        }
    }

    /// Write version manifest to disk (async)
    pub async fn set_version_manifest_to_disk(
        &self,
        name: &str,
        version: &str,
        manifest: &ModelVersionManifest,
    ) {
        let manifest_file = get_package_manifest_cache_file(name, version);

        // Ensure directory exists
        if let Some(parent) = manifest_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let version_manifest = VersionManifest {
            manifest: serde_json::to_value(manifest).unwrap_or_else(|e| {
                log_verbose(&format!(
                    "Failed to serialize manifest for {name}@{version}: {e}"
                ));
                serde_json::json!({})
            }),
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        match serde_json::to_string_pretty(&version_manifest) {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(&manifest_file, content).await {
                    log_verbose(&format!(
                        "Failed to write manifest file for {name}@{version}: {e}"
                    ));
                } else {
                    log_verbose(&format!("Wrote manifest to disk: {name}@{version}"));
                }
            }
            Err(e) => {
                log_verbose(&format!(
                    "Failed to serialize manifest for {name}@{version}: {e}"
                ));
            }
        }
    }

    /// Manually flush cache to disk (for graceful shutdown)
    pub async fn flush_to_disk(&self) -> Result<()> {
        log_verbose("Cache flush: Per-package manifests are written asynchronously");
        Ok(())
    }

    // === Project-level cache methods (for backward compatibility) ===

    pub async fn export_data(&self) -> CacheData {
        let cache = self.project_cache.read().await;
        CacheData {
            cache: cache.clone(),
        }
    }

    pub async fn import_data(&self, data: CacheData) {
        let mut cache = self.project_cache.write().await;
        *cache = data.cache;
    }

    // === Project-level cache methods (for dependency resolution state) ===
    pub async fn get_manifest_in_project_cache(
        &self,
        name: &str,
        _spec: &str,
        version: &str,
    ) -> Option<Value> {
        let cache = self.project_cache.read().await;
        if let Some(manifest) = cache
            .get(name)
            .and_then(|(_, versions)| versions.get(version))
            .cloned()
        {
            return Some(manifest);
        }
        None
    }

    pub async fn set_manifest_in_project_cache(
        &self,
        name: &str,
        spec: &str,
        version: &str,
        manifest: Value,
    ) {
        let mut cache = self.project_cache.write().await;
        let (specs, versions) = cache
            .entry(name.to_string())
            .or_insert_with(|| (HashMap::new(), HashMap::new()));
        specs.insert(spec.to_string(), version.to_string());
        versions.insert(version.to_string(), manifest.clone());
    }

    pub async fn get_version_in_project_cache(&self, name: &str, spec: &str) -> Option<String> {
        let cache = self.project_cache.read().await;
        cache
            .get(name)
            .and_then(|(specs, _)| specs.get(spec))
            .cloned()
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
