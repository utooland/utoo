use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest;
use serde_json::Value;
use std::time::Instant;
use tokio_retry::RetryIf;

use super::cache::PACKAGE_CACHE;
use crate::util::config::{get_registry, get_registry_support_abbr, get_registry_support_semver};
use crate::util::logger::{log_error, log_verbose};
use crate::util::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};

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
        if let Some(stripped) = etag.strip_prefix("W/") {
            stripped.to_string()
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

    /// Get package versions info with caching (name) => (version_list, dist-tags)
    /// Cache stored at ~/.cache/nm/<name>/versions.json

    /// Get specific version manifest with caching (name, version) => manifest
    /// Cache stored at ~/.cache/nm/<name>/manifests/<version>.json
    pub async fn get_version_manifest(&self, name: &str, version: &str) -> Result<Value> {
        // First try to get from versions cache (if we have the full manifest)
        if !get_registry_support_semver()
            && let Some((versions, _)) = self.try_get_package_versions_cached(name).await
            && let Some(manifest) = versions.get(version)
        {
            log_verbose(&format!(
                "Found {name}@{version} manifest in versions cache"
            ));
            return Ok(manifest.clone());
        }

        // Try to load from manifest cache file directly
        if let Some(manifest) = self.load_version_manifest_from_cache(name, version).await {
            log_verbose(&format!("Loaded {name}@{version} manifest from cache file"));
            return Ok(manifest);
        }

        // Fallback: fetch from registry
        log_verbose(&format!("Fetching {name}@{version} manifest from registry"));
        let url = self.build_url(name, version);


        let manifest: Value = RetryIf::spawn(
            create_retry_strategy(),
            || async {

                let accept = match get_registry_support_abbr() {
                    true => "application/json",
                    false => "application/vnd.npm.install-v1+json",
                };

                let response = self
                    .client
                    .get(&url)
                    .header("Accept", accept)
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
                    log_error(&format!("HTTP error: {response:?}, url: {url}"));
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
        PACKAGE_CACHE
            .cache_version_manifest(name, version, &manifest)
            .await;

        Ok(manifest)
    }


    /// Try to get cached package versions without triggering network requests
    async fn try_get_package_versions_cached(&self, name: &str) -> Option<(Value, Value)> {
        let cached_info = PACKAGE_CACHE.get_full_manifests(name).await?;
        let versions = cached_info
            .data
            .get("versions")
            .cloned()
            .unwrap_or_default();
        let dist_tags = cached_info
            .data
            .get("dist-tags")
            .cloned()
            .unwrap_or_default();
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
                        log_verbose(&format!(
                            "Failed to parse manifest file for {name}@{version}: {e}"
                        ));
                        None
                    }
                }
            }
            Err(e) => {
                log_verbose(&format!(
                    "Failed to read manifest file for {name}@{version}: {e}"
                ));
                None
            }
        }
    }

    /// Get complete package information with etag caching
    pub async fn get_full_manifest(&self, name: &str) -> Result<Value> {
        let start = Instant::now();
        // Check cache first to get existing etag
        let cached_info = PACKAGE_CACHE.get_full_manifests(name).await;

        log_verbose(&format!(
            "Using cached package info for {name}, try cache took {:?}",
            start.elapsed()
        ));

        // Build request URL for complete package info
        let url = format!("{}/{}", self.base_url, name);

        // Create request with conditional headers if we have cached data
        let mut request_builder = self.client.get(&url);
        if let Some(ref info) = cached_info
            && let Some(ref etag) = info.etag
        {
            request_builder = request_builder
                .header("If-None-Match", etag)
                .header("Accept", "application/vnd.npm.install-v1+json");
            log_verbose(&format!(
                "Making conditional request for {name} with ETag: {etag}"
            ));
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
                        let duration = start_time.elapsed();
                        log_verbose(&format!("304 Not Modified for {name}, cache is valid {response:?}, took {duration:?}"));
                        Ok(None) // Signal to use cached data
                    }
                    200..=299 => {
                        let etag = response.headers()
                            .get("etag")
                            .and_then(|h| h.to_str().ok())
                            .map(Self::normalize_etag);

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
                PACKAGE_CACHE
                    .set_package_info(name, package_info.clone(), etag)
                    .await;
                Ok(package_info)
            }
            None => {
                // 304 Not Modified, use cached data
                if let Some(info) = cached_info {
                    Ok(info.data)
                } else {
                    // This shouldn't happen - we got 304 but no cached data
                    Err(anyhow::anyhow!(
                        "Received 304 Not Modified but no cached data available"
                    ))
                }
            }
        }
    }

    /// Get version manifest by full versions - step by step approach:
    /// 1. get_full_manifest(name) to get versions info
    /// 2. find matching version from versions list using semver/dist-tags
    /// 3. call get_version_manifest(name, resolved_version) to get specific manifest
    pub async fn get_version_manifest_by_full_versions(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<Value> {
        use crate::util::semver;

        // Step 1: Get full manifest (包含 versions 和 dist-tags)
        let full_manifest = self.get_full_manifest(name).await?;

        // Extract version list and dist-tags
        let version_list: Vec<String> = match full_manifest.get("versions") {
            Some(versions) => {
                if let Some(versions_obj) = versions.as_object() {
                    versions_obj.keys().cloned().collect()
                } else if let Some(versions_array) = versions.as_array() {
                    versions_array
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                } else {
                    return Err(anyhow::anyhow!("Invalid versions format for {name}"));
                }
            }
            None => return Err(anyhow::anyhow!("No versions found for {name}")),
        };

        let dist_tags = full_manifest.get("dist-tags").cloned().unwrap_or_default();

        // Step 2: Find matching version from versions list
        let version_str = if let Some(dist_tags_obj) = dist_tags.as_object() {
            // Check if spec matches a dist-tag first
            if let Some(tag_version) = dist_tags_obj.get(spec).and_then(|v| v.as_str()) {
                log_verbose(&format!("Found dist-tag {spec} -> {tag_version} for {name}"));
                tag_version.to_string()
            } else {
                // Use semver matching if not a direct tag match
                semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                    .map(|v| v.to_string())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No version found matching {name}@{spec}, available versions: {}",
                            version_list.join(", ")
                        )
                    })?
            }
        } else {
            // No dist-tags, just use semver matching
            semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                .map(|v| v.to_string())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No version found matching {name}@{spec}, available versions: {}",
                        version_list.join(", ")
                    )
                })?
        };

        // Step 3: Get the specific version manifest
        let manifest = self.get_version_manifest(name, &version_str).await?;

        log_verbose(&format!("Resolved {name}@{spec} => {version_str}"));

        Ok(manifest)
    }
}

// Global HTTP client access functions - 3 core methods

/// Get complete package information with all versions and metadata
pub async fn get_full_manifest(name: &str) -> Result<Value> {
    REGISTRY_CLIENT.get_full_manifest(name).await
}

/// Get specific version manifest by exact version
pub async fn get_version_manifest(name: &str, version: &str) -> Result<Value> {
    REGISTRY_CLIENT.get_version_manifest(name, version).await
}

/// Get version manifest using full versions approach:
/// 1. Get full manifest with all versions
/// 2. Resolve version from spec using semver/dist-tags
/// 3. Get specific version manifest
pub async fn get_version_manifest_by_full_versions(name: &str, spec: &str) -> Result<Value> {
    REGISTRY_CLIENT
        .get_version_manifest_by_full_versions(name, spec)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

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
