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
        let package_info: Value =
            RetryIf::spawn(
                create_retry_strategy(),
                || async {
                    let response =
                        self.client.get(&url).send().await.map_err(|e| {
                            RetryableError::Temporary(format!("Network error: {e}"))
                        })?;

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
        log_verbose(&format!(
            "HTTP request for package info {name} took {duration:?}"
        ));

        Ok(package_info)
    }
}

// Global HTTP client access functions
pub async fn get_package_manifest(name: &str, spec: &str) -> Result<(String, Value)> {
    REGISTRY_CLIENT.get_package_manifest(name, spec).await
}

pub async fn get_package_info(name: &str) -> Result<Value> {
    REGISTRY_CLIENT.get_package_info(name).await
}
