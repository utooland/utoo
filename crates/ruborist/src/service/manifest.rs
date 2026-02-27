//! Manifest fetching with retry for registry operations.
//!
//! Uses `tokio_retry` with fixed delays for transient error recovery.
//! Retryable errors are identified structurally (HTTP status codes and
//! `reqwest::Error` type checks), not by string matching.

use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio_retry::RetryIf;

use super::http::get_client;
use crate::model::manifest::{FullManifest, VersionManifest};

/// Fixed retry delays.
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

fn retry_strategy() -> impl Iterator<Item = Duration> {
    RETRY_DELAYS.into_iter()
}

/// A manifest fetch error that knows whether it should be retried.
#[derive(Debug)]
enum FetchError {
    /// Transient error (network, timeout, 5xx, 429) -- worth retrying.
    Retryable(anyhow::Error),
    /// Permanent error (404, JSON parse, 304) -- do not retry.
    Permanent(anyhow::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Retryable(e) | FetchError::Permanent(e) => e.fmt(f),
        }
    }
}

/// Classify a `reqwest::Error` as retryable or permanent.
///
/// - `is_timeout()` / `is_connect()`: transient network issues
/// - `is_body()`: stream read errors (e.g. connection reset mid-transfer)
/// - Everything else (including `is_request()` for invalid URLs): permanent
fn classify_reqwest_error(e: reqwest::Error) -> FetchError {
    if e.is_timeout() || e.is_connect() || e.is_body() {
        FetchError::Retryable(anyhow!(e))
    } else {
        FetchError::Permanent(anyhow!(e))
    }
}

/// Classify an HTTP status code as retryable or permanent.
fn classify_status(status: reqwest::StatusCode, url: &str) -> FetchError {
    match status.as_u16() {
        304 => FetchError::Permanent(anyhow!("Not modified")),
        404 => FetchError::Permanent(anyhow!("Package not found")),
        429 => {
            tracing::warn!("HTTP 429 (rate limited) for {}", url);
            FetchError::Retryable(anyhow!("HTTP 429"))
        }
        s if (500..600).contains(&s) => {
            tracing::warn!("HTTP {} for {}", status, url);
            FetchError::Retryable(anyhow!("HTTP {}", status))
        }
        _ => {
            tracing::warn!("HTTP {} for {}", status, url);
            FetchError::Permanent(anyhow!("HTTP {}", status))
        }
    }
}

fn is_retryable(err: &FetchError) -> bool {
    matches!(err, FetchError::Retryable(_))
}

/// Fetch full manifest with retry and ETag support.
pub async fn fetch_full_manifest(
    registry_url: &str,
    name: &str,
    use_abbreviated: bool,
    etag: Option<&str>,
) -> Result<(FullManifest, Option<String>)> {
    let url = format!("{}/{}", registry_url, name);
    let etag_owned = etag.map(|s| s.to_string());

    tracing::debug!("Fetching full manifest for {} from {}", name, url);

    RetryIf::spawn(
        retry_strategy(),
        || {
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

                let response = request.send().await.map_err(classify_reqwest_error)?;

                if response.status().is_success() {
                    let new_etag = response
                        .headers()
                        .get("etag")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());

                    let manifest: FullManifest = response
                        .json()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("JSON parse error: {e}")))?;

                    Ok((manifest, new_etag))
                } else {
                    Err(classify_status(response.status(), &url))
                }
            }
        },
        is_retryable,
    )
    .await
    .map_err(|e| match e {
        FetchError::Retryable(e) | FetchError::Permanent(e) => {
            anyhow!("Failed to fetch {}: {}", name, e)
        }
    })
}

/// Fetch version manifest with retry.
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

    RetryIf::spawn(
        retry_strategy(),
        || {
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
                    .map_err(classify_reqwest_error)?;

                if response.status().is_success() {
                    response
                        .json()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("JSON parse error: {e}")))
                } else {
                    Err(classify_status(response.status(), &url))
                }
            }
        },
        is_retryable,
    )
    .await
    .map_err(|e| match e {
        FetchError::Retryable(e) | FetchError::Permanent(e) => {
            anyhow!("Failed to fetch {}@{}: {}", name, spec, e)
        }
    })
}
