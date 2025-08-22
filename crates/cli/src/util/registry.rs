use super::retry::build_dns_cached_client;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_retry::RetryIf;

use crate::util::node::EdgeType;

use super::config::get_registry;
use super::logger::log_verbose;
use super::retry::{RetryableError, create_retry_strategy};

pub static PACKAGE_CACHE: Lazy<PackageCache> = Lazy::new(PackageCache::new);

// Modified cache structure definition
type VersionMap = HashMap<String, Value>;
type SpecMap = HashMap<String, String>; // spec -> version
type CacheMap = HashMap<String, (SpecMap, VersionMap)>; // name -> (specs, versions)

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

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, String>,
}

pub struct Registry {
    client: reqwest::Client,
    base_url: String,
}

// Global Registry instance
static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    #[allow(dead_code)]
    pub name: String,
    pub manifest: Value,
    pub version: String,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            client: build_dns_cached_client(),
            base_url: get_registry().to_string(),
        }
    }

    #[cfg(test)]
    pub fn new_with(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }

    fn build_url(&self, name: &str, spec: &str) -> String {
        if spec.starts_with("npm:") {
            let npm_spec = spec.strip_prefix("npm:").unwrap();
            // Handle npm spec format: npm:@babel/traverse@^7.25.3 => @babel/traverse/^7.25.3
            if let Some(last_at_index) = npm_spec.rfind('@') {
                let (pkg_name, version) = npm_spec.split_at(last_at_index);
                return format!("{}/{}/{}", self.base_url, pkg_name, &version[1..]);
            }
        }

        if spec.starts_with("workspace:") {
            let workspace_spec = spec.strip_prefix("workspace:").unwrap();
            return format!("{}/{}/{}", self.base_url, name, workspace_spec);
        }

        if spec.eq("*") {
            return format!("{}/{}/latest", self.base_url, name);
        }

        format!("{}/{}/{}", self.base_url, name, spec)
    }

    pub async fn get_package_manifest(&self, name: &str, spec: &str) -> Result<(String, Value)> {
        // First check cache for version
        if let Some(version) = PACKAGE_CACHE.get_version(name, spec).await
            && let Some(manifest) = PACKAGE_CACHE.get_manifest(name, spec, &version).await
        {
            log_verbose(&format!("Cache hit for {name}@{spec} => {version}"));
            return Ok((version, manifest));
        }

        // Build request URL
        let url = self.build_url(name, spec);

        // Record start time
        let start_time = Instant::now();

        // Retry HTTP request with custom strategy
        let manifest: Value = RetryIf::spawn(
            create_retry_strategy(),
            || async {
                let response = self
                    .client
                    .get(&url)
                    .header("Accept", "application/vnd.npm.install-v1+json")
                    .send()
                    .await
                    .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

                if response.status().is_success() {
                    let manifest = response.json().await.map_err(|e| {
                        RetryableError::Temporary(format!("Failed to parse JSON response: {e}"))
                    })?;
                    Ok(manifest)
                } else if response.status().as_u16() == 404 {
                    log_verbose(&format!("URL not found {url}"));
                    Err(RetryableError::Permanent(format!(
                        "Fetch Error: {}, status: {}",
                        url,
                        response.status()
                    )))
                } else {
                    log_verbose(&format!(
                        "HTTP error: url: {}, status: {}, retrying",
                        url,
                        response.status()
                    ));
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {}, url: {}",
                        response.status(),
                        url
                    )))
                }
            },
            |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch manifest after retries: {e}"))?;

        // Calculate and log request duration
        let duration = start_time.elapsed();
        log_verbose(&format!("HTTP request for {name}@{spec} took {duration:?}"));

        // Extract version
        let version = match manifest.get("version").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => {
                log_verbose(&format!("Invalid manifest: {manifest:?}"));
                return Err(anyhow::anyhow!("Invalid manifest: missing version"));
            }
        };

        // Update cache
        PACKAGE_CACHE
            .set_manifest(name, spec, &version, manifest.clone())
            .await;

        Ok((version, manifest))
    }

    /// Get complete package information (like npm view)
    pub async fn get_package_info(&self, name: &str) -> Result<Value> {
        // Build request URL for complete package info
        let url = format!("{}/{}", self.base_url, name);
        log_verbose(&format!("Fetching package info at {url}"));

        // Record start time
        let start_time = Instant::now();

        // Retry HTTP request with custom strategy
        let package_info: Value = RetryIf::spawn(
            create_retry_strategy(),
            || async {
                let response = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

                if response.status().is_success() {
                    let package_info = response.json().await.map_err(|e| {
                        RetryableError::Temporary(format!("Failed to parse JSON response: {e}"))
                    })?;
                    Ok(package_info)
                } else if response.status().as_u16() == 404 {
                    log_verbose(&format!("URL not found {url}"));
                    Err(RetryableError::Permanent(format!(
                        "Fetch Error: {}, status: {}",
                        url,
                        response.status()
                    )))
                } else {
                    log_verbose(&format!(
                        "HTTP error: url: {}, status: {}, retrying",
                        url,
                        response.status()
                    ));
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {}, url: {}",
                        response.status(),
                        url
                    )))
                }
            },
            |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch package info after retries: {e}"))?;

        // Calculate and log request duration
        let duration = start_time.elapsed();
        log_verbose(&format!("HTTP request for package info {name} took {duration:?}"));

        Ok(package_info)
    }

    async fn resolve_package(&self, name: &str, spec: &str) -> Result<ResolvedPackage> {
        let (version, mut manifest) = self.get_package_manifest(name, spec).await?;
        log_verbose(&format!("Resolved {name}@{spec} => {version}"));
        if let Some(obj) = manifest.as_object_mut() {
            // merge dependencies and devDependencies
            if let Some(optional_deps) = obj.get("optionalDependencies").and_then(|v| v.as_object())
            {
                let optional_keys: Vec<String> = optional_deps.keys().cloned().collect();
                if let Some(deps) = obj.get_mut("dependencies").and_then(|v| v.as_object_mut()) {
                    for key in &optional_keys {
                        deps.remove(key);
                    }
                }
                if let Some(dev_deps) = obj
                    .get_mut("devDependencies")
                    .and_then(|v| v.as_object_mut())
                {
                    for key in &optional_keys {
                        dev_deps.remove(key);
                    }
                }
            }
        }

        Ok(ResolvedPackage {
            name: name.to_string(),
            version,
            manifest,
        })
    }
}

// Global resolve function
pub async fn resolve(name: &str, spec: &str) -> Result<ResolvedPackage> {
    REGISTRY.resolve_package(name, spec).await
}

// Global package info function
pub async fn get_package_info(name: &str) -> Result<Value> {
    REGISTRY.get_package_info(name).await
}

// Public cache operations
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
    // Check file existence
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

pub async fn resolve_dependency(
    name: &str,
    spec: &str,
    edge_type: &EdgeType,
) -> Result<Option<ResolvedPackage>> {
    match resolve(name, spec).await {
        Ok(resolved) => Ok(Some(resolved)),
        Err(e) => {
            if *edge_type == EdgeType::Optional {
                log_verbose(&format!(
                    "skipping optional dependency {name}@{spec} due to resolve error: {e}"
                ));
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_resolve_package() -> Result<()> {
        let result = resolve("lodash", "^4").await?;

        assert!(result.version.starts_with("4"));
        assert_eq!(result.name, "lodash");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_package_manifest() -> Result<()> {
        let registry = Registry::new();

        // Test fetching lodash manifest
        let (version, manifest) = registry.get_package_manifest("lodash", "^4").await?;

        assert!(version.starts_with("4"));
        assert_eq!(manifest["name"], "lodash");

        // Verify cache update
        let cached_manifest = PACKAGE_CACHE
            .get_manifest("lodash", "4.17.21", "4.17.21")
            .await;
        assert!(cached_manifest.is_some());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_package_manifest_from_cache() -> Result<()> {
        let registry = Registry::new();

        // First request
        let (version1, manifest1) = registry.get_package_manifest("lodash", "4.17.21").await?;

        // Second request (should hit cache)
        let (version2, manifest2) = registry.get_package_manifest("lodash", "4.17.21").await?;

        assert_eq!(version1, version2);
        assert_eq!(manifest1, manifest2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_package_manifest_not_found() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/not-exist-package-12345/1.0.0")
            .with_status(404)
            .create_async()
            .await;
        let registry = Registry::new_with(reqwest::Client::new(), server.url());
        let result = registry
            .get_package_manifest("not-exist-package-12345", "1.0.0")
            .await;
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Fetch Error"));
        }
    }

    #[tokio::test]
    async fn test_get_package_manifest_invalid_version() {
        let mut server = Server::new_async().await;
        // Return 404 for this version
        let _m = server
            .mock("GET", "/lodash/999.999.999")
            .with_status(404)
            .create_async()
            .await;
        let registry = Registry::new_with(reqwest::Client::new(), server.url());
        let result = registry.get_package_manifest("lodash", "999.999.999").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_dependency_normal() -> Result<()> {
        // Test resolving a normal dependency
        let result = resolve_dependency("lodash", "^4", &EdgeType::Prod).await?;
        assert!(result.is_some());
        let pkg = result.unwrap();
        assert_eq!(pkg.name, "lodash");
        assert!(pkg.version.starts_with("4"));
        Ok(())
    }

    #[tokio::test]
    async fn test_resolve_dependency_optional_not_found() -> Result<()> {
        // Should skip and return Ok(None) for optional dependency not found
        let result =
            resolve_dependency("not-exist-package-12345", "1.0.0", &EdgeType::Optional).await?;
        assert!(result.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn test_resolve_dependency_regular_not_found() {
        // Should return Err for regular dependency not found
        let result = resolve_dependency("not-exist-package-12345", "1.0.0", &EdgeType::Prod).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_dependency_invalid_version() {
        // Should return Err for invalid version
        let result = resolve_dependency("lodash", "999.999.999", &EdgeType::Prod).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_package_manifest_404_no_retry() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/lodash/1.0.0")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let registry = Registry::new_with(reqwest::Client::new(), server.url());
        let result = registry.get_package_manifest("lodash", "1.0.0").await;
        assert!(result.is_err());
        let err_str = format!("{}", result.unwrap_err());
        assert!(err_str.contains("Fetch Error"));
    }

    #[tokio::test]
    async fn test_get_package_manifest_5xx_retry_then_success() {
        let mut server = Server::new_async().await;
        let _m1 = server
            .mock("GET", "/lodash/2.0.0")
            .with_status(500)
            .expect(1)
            .create_async()
            .await;
        let _m2 = server
            .mock("GET", "/lodash/2.0.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"name":"lodash","version":"2.0.0"}"#)
            .expect(1)
            .create_async()
            .await;
        let registry = Registry::new_with(reqwest::Client::new(), server.url());
        let result = registry.get_package_manifest("lodash", "2.0.0").await;
        assert!(result.is_ok());
        let (version, manifest) = result.unwrap();
        assert_eq!(version, "2.0.0");
        assert_eq!(manifest["name"], "lodash");
    }

    #[test]
    fn test_build_url_workspace_protocol() {
        // Use a dummy base_url for testing
        let base_url = "http://registry.example.com".to_string();
        let client = reqwest::Client::new();
        let registry = Registry::new_with(client, base_url.clone());
        let name = "my-pkg";
        let spec = "workspace:1.2.3";
        let url = registry.build_url(name, spec);
        // Should produce: http://localhost:4873/my-pkg/1.2.3
        assert_eq!(url, format!("{}/{}/{}", base_url, name, "1.2.3"));
    }
}
