//! HTTP client for registry operations.
//!
//! Provides retry with exponential backoff for both native and WASM targets.

use anyhow::{Result, anyhow};
use std::env;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::model::manifest::{FullManifest, VersionManifest};
use crate::service::dns::CachingResolver;

/// Global HTTP client with connection pooling and DNS caching
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn get_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Arc::new(CachingResolver::new(Duration::from_secs(300))));

        builder = match env_proxy() {
            Some(EnvProxy::All(url)) => {
                builder.proxy(reqwest::Proxy::all(&url).expect("invalid ALL_PROXY url"))
            }
            Some(EnvProxy::PerScheme { https, http }) => {
                if let Some(url) = https {
                    builder = builder
                        .proxy(reqwest::Proxy::https(&url).expect("invalid HTTPS_PROXY url"));
                }
                if let Some(url) = http {
                    builder = builder
                        .proxy(reqwest::Proxy::http(&url).expect("invalid HTTP_PROXY url"));
                }
                builder
            }
            None => builder,
        };

        builder.build().expect("Failed to build reqwest client")
    })
}

enum EnvProxy {
    All(String),
    PerScheme {
        https: Option<String>,
        http: Option<String>,
    },
}

fn env_proxy() -> Option<EnvProxy> {
    if let Some(url) = env_var("ALL_PROXY") {
        return Some(EnvProxy::All(url));
    }
    let https = env_var("HTTPS_PROXY");
    let http = env_var("HTTP_PROXY");
    if https.is_some() || http.is_some() {
        return Some(EnvProxy::PerScheme { https, http });
    }
    None
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).or_else(|_| env::var(key.to_lowercase())).ok()
}

/// Check if an error is retryable.
/// Only retry on network errors or 5xx/429 server errors.
fn is_retryable_error(err: &anyhow::Error) -> bool {
    let err_str = err.to_string();
    // Retry on network/connection errors
    if err_str.contains("Network error")
        || err_str.contains("timeout")
        || err_str.contains("connection")
    {
        return true;
    }
    // Retry on 5xx server errors and 429 rate limiting
    if err_str.contains("HTTP 5") || err_str.contains("HTTP 429") {
        return true;
    }
    // Don't retry 3xx, 4xx (except 429), or other errors
    false
}

/// Simple retry with exponential backoff.
/// Only retries on network errors, 5xx server errors, and 429 rate limiting.
async fn with_retry<T, F, Fut>(max_retries: usize, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let delays = [100, 200, 500, 1000, 2000];
    let mut last_error = anyhow!("No attempts made");

    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                // Only retry on retryable errors
                if !is_retryable_error(&e) {
                    return Err(e);
                }
                tracing::debug!("Retryable error (attempt {}): {}", attempt + 1, e);
                last_error = e;
                if attempt < max_retries {
                    let delay_ms = delays.get(attempt).copied().unwrap_or(2000);
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms as u64)).await;
                }
            }
        }
    }

    Err(last_error)
}

/// Fetch full manifest with retry and ETag support
pub async fn fetch_full_manifest(
    registry_url: &str,
    name: &str,
    use_abbreviated: bool,
    etag: Option<&str>,
) -> Result<(FullManifest, Option<String>)> {
    let url = format!("{}/{}", registry_url, name);
    let etag_owned = etag.map(|s| s.to_string());

    tracing::debug!("Fetching full manifest for {} from {}", name, url);

    with_retry(5, || {
        let url = url.clone();
        let etag = etag_owned.clone();
        async move {
            let accept = if use_abbreviated {
                "application/vnd.npm.install-v1+json"
            } else {
                "application/json"
            };

            let mut request = get_client().get(&url).header("Accept", accept);
            if let Some(etag_value) = &etag {
                request = request.header("If-None-Match", etag_value);
            }

            let response = request
                .send()
                .await
                .map_err(|e| anyhow!("Network error: {e}"))?;

            if response.status().is_success() {
                let new_etag = response
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                let manifest: FullManifest = response
                    .json()
                    .await
                    .map_err(|e| anyhow!("JSON parse error: {e}"))?;

                Ok((manifest, new_etag))
            } else if response.status().as_u16() == 304 {
                Err(anyhow!("Not modified"))
            } else if response.status().as_u16() == 404 {
                Err(anyhow!("Package not found"))
            } else {
                tracing::warn!("HTTP error for {}: {}", url, response.status());
                Err(anyhow!("HTTP {}", response.status()))
            }
        }
    })
    .await
    .map_err(|e| anyhow!("Failed to fetch {}: {}", name, e))
}

/// Fetch version manifest with retry
///
/// `use_abbreviated`: Whether to use abbreviated manifest format.
/// Only semver-supporting registries (npmmirror) support this.
/// For npm registry, use false to get standard JSON format.
pub async fn fetch_version_manifest(
    registry_url: &str,
    name: &str,
    spec: &str,
    use_abbreviated: bool,
) -> Result<VersionManifest> {
    let url = format!("{}/{}/{}", registry_url, name, spec);

    tracing::debug!(
        "Fetching version manifest for {}@{} from {}",
        name,
        spec,
        url
    );

    with_retry(5, || {
        let url = url.clone();
        async move {
            let accept = if use_abbreviated {
                "application/vnd.npm.install-v1+json"
            } else {
                "application/json"
            };

            let response = get_client()
                .get(&url)
                .header("Accept", accept)
                .send()
                .await
                .map_err(|e| anyhow!("Network error: {e}"))?;

            if response.status().is_success() {
                response
                    .json()
                    .await
                    .map_err(|e| anyhow!("JSON parse error: {e}"))
            } else if response.status().as_u16() == 404 {
                Err(anyhow!("Package not found"))
            } else {
                tracing::warn!("HTTP error for {}: {}", url, response.status());
                Err(anyhow!("HTTP {}", response.status()))
            }
        }
    })
    .await
    .map_err(|e| anyhow!("Failed to fetch {}@{}: {}", name, spec, e))
}
