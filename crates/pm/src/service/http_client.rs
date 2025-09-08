use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest;
use serde_json::Value;
use std::time::Instant;
use tokio_retry::RetryIf;

use super::cache::PACKAGE_CACHE;
use crate::util::config::get_registry;
use crate::util::logger::log_verbose;
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

        if cached_info.is_some() {
            log_verbose(&format!(
                "Using cached package info for {name}, fetched in {:?}",
                start.elapsed()
            ));
            return Ok(cached_info.unwrap().data);
        }

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

    /// Get package manifest using complete package info and semver matching
    pub async fn get_package_manifest_with_semver(&self, name: &str, spec: &str) -> Result<(String, Value)> {
        // First check cache for version
        if let Some(version) = PACKAGE_CACHE.get_version(name, spec).await
            && let Some(manifest) = PACKAGE_CACHE.get_manifest(name, spec, &version).await
        {
            log_verbose(&format!("Cache hit for {name}@{spec} => {version}"));
            return Ok((version, manifest));
        }

        // Get complete package info
        let package_info = self.get_package_info(name).await?;

        // Extract versions
        let versions = package_info
            .get("versions")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("Invalid package info: missing versions {package_info:?}"))?;

        // Find the best matching version using semver
        let version_str = if spec == "*" || spec == "latest" {
            // Get latest version
            package_info
                .get("dist-tags")
                .and_then(|tags| tags.get("latest"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // Fallback to highest version
                    max_satisfying(versions.keys().map(|k| k.as_str()), "*")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "latest".to_string())
                })
        } else {
            // Use semver matching
            max_satisfying(versions.keys().map(|k| k.as_str()), spec)
                .map(|v| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("No version found matching {spec}, {versions:?}"))?
        };

        // Get the manifest for this version
        let manifest = versions
            .get(&version_str)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Version {version_str} not found in package info"))?;

        log_verbose(&format!("Resolved {name}@{spec} => {version_str}"));

        // Update cache
        PACKAGE_CACHE
            .set_manifest(name, spec, &version_str, manifest.clone())
            .await;

        // Cache the resolved version manifest for future direct access
        PACKAGE_CACHE
            .cache_version_manifest(name, &version_str, &manifest)
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
