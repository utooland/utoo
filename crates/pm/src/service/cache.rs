use std::collections::HashMap;
use std::path::Path;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use std::sync::Arc;

use crate::util::logger::log_verbose;

type VersionMap = HashMap<String, Value>;
type SpecMap = HashMap<String, String>; // spec -> version
type CacheMap = HashMap<String, (SpecMap, VersionMap)>; // name -> (specs, versions)

pub static PACKAGE_CACHE: Lazy<PackageCache> = Lazy::new(PackageCache::new);

#[derive(Debug)]
pub struct PackageCache {
    cache: Arc<RwLock<CacheMap>>,
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
        }
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