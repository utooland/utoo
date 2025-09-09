use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest;
use serde_json::Value;
use std::time::Instant;
use tokio_retry::RetryIf;

use super::cache::PACKAGE_CACHE;
use crate::util::config::get_registry;
use crate::util::logger::{log_error, log_verbose};
use crate::util::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};
use crate::util::semver::max_satisfying;

pub struct RegistryHttpClient {
    client: reqwest::Client,
    base_url: String,
}

static REGISTRY_CLIENT: Lazy<RegistryHttpClient> = Lazy::new(RegistryHttpClient::new);

impl Default for RegistryHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryHttpClient {
    pub fn new() -> Self {
        Self {
            client: build_dns_cached_client(),
            base_url: get_registry().to_string(),
        }
    }

    /// Normalize ETag by removing weak cache prefix but keep quotes for HTTP compatibility
    fn normalize_etag(etag: &str) -> String {
        let etag = etag.trim();
        // Remove W/ prefix for weak ETags but keep the rest intact
        if etag.starts_with("W/") {
            etag[2..].to_string()
        } else {
            etag.to_string()
        }
    }


    fn build_url(&self, name: &str, spec: &str) -> String {
        if spec.starts_with("npm:") {
            let npm_spec = spec.strip_prefix("npm:").unwrap();
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

        if spec.starts_with("npm:") {
            let npm_spec = spec.strip_prefix("npm:").unwrap();
            if let Some(last_at_index) = npm_spec.rfind('@') {
                let (pkg_name, version) = npm_spec.split_at(last_at_index);
                return self.get_package_manifest_with_semver(pkg_name, &version[1..]).await;
            }
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

        // Cache the resolved version manifest for future direct access
        PACKAGE_CACHE
            .cache_version_manifest(name, &version, &manifest)
            .await;

        Ok((version, manifest))
    }

    /// Get package versions info with caching (name) => (version_list, dist-tags)
    /// Cache stored at ~/.cache/nm/<name>/versions.json
    pub async fn get_package_versions(&self, name: &str) -> Result<(Vec<String>, Value)> {
        // Use get_package_info which handles caching internally to avoid double cache access
        let package_info = self.get_package_info(name).await?;

        let version_list: Vec<String> = match package_info.get("versions") {
            Some(versions) => {
                if let Some(versions_obj) = versions.as_object() {
                    // Standard npm registry format: versions is an object with version keys
                    versions_obj.keys().cloned().collect()
                } else if let Some(versions_array) = versions.as_array() {
                    // Alternative format: versions is an array of version strings
                    versions_array
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    log_verbose(&format!("Unexpected versions format for {}: {:?}", name, versions));
                    Vec::new()
                }
            }
            None => {
                log_verbose(&format!("No versions field found for package {}", name));
                Vec::new()
            }
        };
        let dist_tags = package_info.get("dist-tags").cloned().unwrap_or_default();

        Ok((version_list, dist_tags))
    }

    /// Get specific version manifest with caching (name, version) => manifest
    /// Cache stored at ~/.cache/nm/<name>/manifests/<version>.json
    pub async fn get_package_version_manifest(&self, name: &str, version: &str) -> Result<Value> {
        // First try to get from versions cache (if we have the full manifest)
        if let Some((versions, _)) = self.try_get_package_versions_cached(name).await {
            if let Some(manifest) = versions.get(version) {
                log_verbose(&format!("Found {name}@{version} manifest in versions cache"));
                return Ok(manifest.clone());
            }
        }

        // Try to load from manifest cache file directly
        if let Some(manifest) = self.load_version_manifest_from_cache(name, version).await {
            log_verbose(&format!("Loaded {name}@{version} manifest from cache file"));
            return Ok(manifest);
        }

        // Fallback: fetch from registry
        log_verbose(&format!("Fetching {name}@{version} manifest from registry"));
        let url = format!("{}/{}/{}", self.base_url, name, version);

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
                    Err(RetryableError::Permanent(format!(
                        "Version {version} not found for package {name}"
                    )))
                } else {
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
        .map_err(|e| anyhow::anyhow!("Failed to fetch version manifest after retries: {e}"))?;

        // Cache the fetched manifest
        PACKAGE_CACHE.cache_version_manifest(name, version, &manifest).await;

        Ok(manifest)
    }

    /// Try to get cached package versions without triggering network requests
    async fn try_get_package_versions_cached(&self, name: &str) -> Option<(Value, Value)> {
        let cached_info = PACKAGE_CACHE.get_package_info(name).await?;
        let versions = cached_info.data.get("versions").cloned().unwrap_or_default();
        let dist_tags = cached_info.data.get("dist-tags").cloned().unwrap_or_default();
        Some((versions, dist_tags))
    }

    /// Load version manifest from cache file directly
    async fn load_version_manifest_from_cache(&self, name: &str, version: &str) -> Option<Value> {
        let manifest_file = crate::util::cache::get_package_manifest_cache_file(name, version);

        if !tokio::fs::try_exists(&manifest_file).await.unwrap_or(false) {
            return None;
        }

        match tokio::fs::read_to_string(&manifest_file).await {
            Ok(content) => {
                match serde_json::from_str::<crate::service::cache::VersionManifest>(&content) {
                    Ok(version_manifest) => Some(version_manifest.manifest),
                    Err(e) => {
                        log_verbose(&format!("Failed to parse manifest file for {name}@{version}: {e}"));
                        None
                    }
                }
            }
            Err(e) => {
                log_verbose(&format!("Failed to read manifest file for {name}@{version}: {e}"));
                None
            }
        }
    }

    /// Get complete package information with etag caching
    pub async fn get_package_info(&self, name: &str) -> Result<Value> {
        let start = Instant::now();
        // Check cache first to get existing etag
        let cached_info = PACKAGE_CACHE.get_package_info(name).await;

        log_verbose(&format!(
            "Using cached package info for {name}, try cache took {:?}",
            start.elapsed()
        ));

        // Build request URL for complete package info
        let url = format!("{}/{}", self.base_url, name);

        // Create request with conditional headers if we have cached data
        let mut request_builder = self.client.get(&url);
        if let Some(ref info) = cached_info {
            if let Some(ref etag) = info.etag {
                request_builder = request_builder.header("If-None-Match", etag)
                .header("Accept", "application/vnd.npm.install-v1+json");
                log_verbose(&format!("Making conditional request for {name} with ETag: {etag}"));
            }
        }

        // if cached_info.is_some() {
        //     log_verbose(&format!(
        //         "Using cached package info for {name}, fetched in {:?}",
        //         start.elapsed()
        //     ));
        //     // return Ok(cached_info.unwrap().data);
        // }

        log_verbose(&format!("Fetching package info at {url}"));

        // Record start time
        let start_time = Instant::now();

        // Retry HTTP request with custom strategy
        let result = RetryIf::spawn(
            create_retry_strategy(),
            || async {
                let response = request_builder
                    .try_clone()
                    .ok_or_else(|| RetryableError::Permanent("Failed to clone request".to_string()))?
                    .header("Accept", "application/vnd.npm.install-v1+json")
                    .send()
                    .await
                    .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

                match response.status().as_u16() {
                    304 => {
                        // Not Modified - cache is still valid
                        log_verbose(&format!("304 Not Modified for {name}, cache is valid"));
                        Ok(None) // Signal to use cached data
                    }
                    200..=299 => {
                        let etag = response.headers()
                            .get("etag")
                            .and_then(|h| h.to_str().ok())
                            .map(|s| Self::normalize_etag(s));

                        log_verbose(&format!("{url} headers: {:?}", response.headers()));
                        log_verbose(&format!("{url} ETag: {etag:?}"));

                        let package_info: Value = response.json().await.map_err(|e| {
                            RetryableError::Temporary(format!("Failed to parse JSON response: {e}"))
                        })?;
                        Ok(Some((package_info, etag)))
                    }
                    404 => {
                        log_verbose(&format!("URL not found {url}"));
                        Err(RetryableError::Permanent(format!(
                            "Fetch Error: {}, status: {}",
                            url,
                            response.status()
                        )))
                    }
                    _ => {
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
                }
            },
            |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch package info after retries: {e}"))?;

        // Calculate and log request duration
        let duration = start_time.elapsed();
        log_verbose(&format!(
            "HTTP request for package info {name} took {duration:?}"
        ));

        match result {
            Some((package_info, etag)) => {
                // New data received, cache it
                PACKAGE_CACHE.set_package_info(name, package_info.clone(), etag).await;
                Ok(package_info)
            }
            None => {
                // 304 Not Modified, use cached data
                if let Some(info) = cached_info {
                    Ok(info.data)
                } else {
                    // This shouldn't happen - we got 304 but no cached data
                    Err(anyhow::anyhow!("Received 304 Not Modified but no cached data available"))
                }
            }
        }
    }

    /// Get package manifest using version cache and semver matching
    pub async fn get_package_manifest_with_semver(&self, name: &str, spec: &str) -> Result<(String, Value)> {
        // First check cache for version
        if let Some(version) = PACKAGE_CACHE.get_version(name, spec).await
            && let Some(manifest) = PACKAGE_CACHE.get_manifest(name, spec, &version).await
        {
            log_verbose(&format!("Cache hit for {name}@{spec} => {version}"));
            return Ok((version, manifest));
        }

        // Get package versions and dist-tags using the new caching system
        let (version_list, dist_tags) = self.get_package_versions(name).await?;

        // Find the best matching version using semver
        let version_str = if spec == "*" || spec == "latest" {
            // Get latest version from dist-tags
            dist_tags
                .get("latest")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // Fallback to highest version
                    max_satisfying(version_list.iter().map(|s| s.as_str()), "*")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "latest".to_string())
                })
        } else {
            // Use semver matching
            max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                .map(|v| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("No version found matching {name}@{spec}, available versions: {}", version_list.join(", ")))?
        };

        // Get the manifest for this version using the new caching system
        let manifest = self.get_package_version_manifest(name, &version_str).await?;

        log_verbose(&format!("Resolved {name}@{spec} => {version_str}"));

        // Update cache for the spec -> version mapping
        PACKAGE_CACHE
            .set_manifest(name, spec, &version_str, manifest.clone())
            .await;

        Ok((version_str, manifest))
    }
}

// Global HTTP client access functions
pub async fn get_package_manifest(name: &str, spec: &str) -> Result<(String, Value)> {
    REGISTRY_CLIENT.get_package_manifest(name, spec).await
}

pub async fn get_package_info(name: &str) -> Result<Value> {
    REGISTRY_CLIENT.get_package_info(name).await
}

pub async fn get_package_manifest_with_semver(name: &str, spec: &str) -> Result<(String, Value)> {
    REGISTRY_CLIENT.get_package_manifest_with_semver(name, spec).await
}

pub async fn get_package_versions(name: &str) -> Result<(Vec<String>, Value)> {
    REGISTRY_CLIENT.get_package_versions(name).await
}

pub async fn get_package_version_manifest(name: &str, version: &str) -> Result<Value> {
    REGISTRY_CLIENT.get_package_version_manifest(name, version).await
}
#[cfg(test)]
mod tests {
    use super::RegistryHttpClient;

    #[test]
    fn test_normalize_etag() {
        // Test weak ETag with quotes - remove W/ but keep quotes
        assert_eq!(
            RegistryHttpClient::normalize_etag("W/\"d25290f0d7ffbd35a836b75992c1b822628ffd32\""),
            "\"d25290f0d7ffbd35a836b75992c1b822628ffd32\""
        );

        // Test strong ETag with quotes - keep as is
        assert_eq!(
            RegistryHttpClient::normalize_etag("\"abc123def456\""),
            "\"abc123def456\""
        );

        // Test ETag without quotes - keep as is
        assert_eq!(
            RegistryHttpClient::normalize_etag("abc123def456"),
            "abc123def456"
        );

        // Test weak ETag without quotes - remove W/
        assert_eq!(
            RegistryHttpClient::normalize_etag("W/abc123def456"),
            "abc123def456"
        );

        // Test with extra whitespace
        assert_eq!(
            RegistryHttpClient::normalize_etag("  W/\"abc123def456\"  "),
            "\"abc123def456\""
        );
    }
}
