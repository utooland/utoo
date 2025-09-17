use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest;
use serde_json::Value;
use std::time::Instant;
use tokio_retry::RetryIf;

use crate::util::config::{get_registry, get_registry_support_abbr};
use crate::util::logger::{log_error, log_verbose};
use crate::util::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};

pub struct RegistryHttpClient {
    client: reqwest::Client,
    base_url: String,
}

static REGISTRY_CLIENT: Lazy<RegistryHttpClient> = Lazy::new(|| {
    let client = build_dns_cached_client();
    let base_url = get_registry().trim_end_matches('/').to_string();
    log_verbose(&format!("Initialized HTTP client with base URL: {}", base_url));

    RegistryHttpClient { client, base_url }
});

impl RegistryHttpClient {
    /// Build URL for package or version requests
    fn build_url(&self, name: &str, spec: &str) -> String {
        if spec == "*" {
            return format!("{}/{}/latest", self.base_url, name);
        }
        format!("{}/{}/{}", self.base_url, name, spec)
    }

    /// Fetch complete package information via HTTP (no caching)
    pub async fn fetch_full_manifest(&self, name: &str, etag: Option<&str>) -> Result<(Value, Option<String>)> {
        let url = format!("{}/{}", self.base_url, name);
        log_verbose(&format!("Fetching full manifest for {name} from {url}"));

        let start = Instant::now();
        let response = RetryIf::spawn(
            create_retry_strategy(),
            || async {
                let mut request = self
                    .client
                    .get(&url)
                    .header("Accept", "application/json");

                // Add If-None-Match header if etag provided
                if let Some(etag_value) = etag {
                    request = request.header("If-None-Match", etag_value);
                }

                let response = request
                    .send()
                    .await
                    .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

                if response.status().is_success() {
                    Ok(response)
                } else if response.status().as_u16() == 304 {
                    // Not Modified - return special marker
                    Err(RetryableError::Permanent("NOT_MODIFIED".to_string()))
                } else if response.status().as_u16() == 404 {
                    Err(RetryableError::Permanent(format!(
                        "Package {name} not found"
                    )))
                } else {
                    log_error(&format!("HTTP error: {response:?}, url: {url}"));
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: status={}, url={}",
                        response.status(),
                        url
                    )))
                }
            },
            |err: &RetryableError| matches!(err, RetryableError::Temporary(_)),
        );

        match response.await {
            Ok(response) => {
                let new_etag = response
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(Self::normalize_etag)
                    .map(|s| s.to_string());

                let data = response.json().await?;
                log_verbose(&format!(
                    "Successfully fetched full manifest for {name} in {:?}",
                    start.elapsed()
                ));
                Ok((data, new_etag))
            }
            Err(e) => {
                if e.to_string().contains("NOT_MODIFIED") {
                    Err(anyhow::anyhow!("Not modified"))
                } else {
                    Err(anyhow::anyhow!("Failed to fetch full manifest after retries: {e}"))
                }
            }
        }
    }

    /// Fetch specific version manifest via HTTP (no caching)
    pub async fn fetch_version_manifest(&self, name: &str, version: &str) -> Result<Value> {
        let url = self.build_url(name, version);
        log_verbose(&format!("Fetching {name}@{version} manifest from {url}"));

        let manifest: Value = RetryIf::spawn(
            create_retry_strategy(),
            || async {
                let accept = if get_registry_support_abbr() {
                    "application/vnd.npm.install-v1+json"
                } else {
                    "application/json"
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
                        "HTTP error: status={}, url={}",
                        response.status(),
                        url
                    )))
                }
            },
            |err: &RetryableError| matches!(err, RetryableError::Temporary(_)),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch version manifest after retries: {e}"))?;

        Ok(manifest)
    }

    /// Normalize ETag value by removing W/ prefix but keeping quotes
    fn normalize_etag(etag: &str) -> &str {
        etag.strip_prefix("W/").unwrap_or(etag)
    }
}

// Global HTTP client access functions - pure HTTP operations only

/// Fetch complete package information via HTTP
pub async fn fetch_full_manifest(name: &str, etag: Option<&str>) -> Result<(Value, Option<String>)> {
    REGISTRY_CLIENT.fetch_full_manifest(name, etag).await
}

/// Fetch specific version manifest via HTTP
pub async fn fetch_version_manifest(name: &str, version: &str) -> Result<Value> {
    REGISTRY_CLIENT.fetch_version_manifest(name, version).await
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
            RegistryHttpClient::normalize_etag("\"d25290f0d7ffbd35a836b75992c1b822628ffd32\""),
            "\"d25290f0d7ffbd35a836b75992c1b822628ffd32\""
        );

        // Test ETag without quotes - keep as is
        assert_eq!(
            RegistryHttpClient::normalize_etag("d25290f0d7ffbd35a836b75992c1b822628ffd32"),
            "d25290f0d7ffbd35a836b75992c1b822628ffd32"
        );
    }
}
