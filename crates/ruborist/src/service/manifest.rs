//! Manifest fetching with retry for registry operations.
//!
//! Uses `tokio_retry` with fixed delays for transient error recovery.
//! Retryable errors are identified structurally (HTTP status codes and
//! `reqwest::Error` type checks), not by string matching.

use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio_retry::RetryIf;

use super::http::get_client;
use crate::model::manifest::{CoreVersionManifest, FullManifest};

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

/// Result of a full manifest fetch with ETag support.
/// Transient return value, immediately destructured — Box not needed.
#[allow(clippy::large_enum_variant)]
pub enum FetchManifestResult {
    /// 200 OK — fresh manifest with optional new ETag.
    Ok(FullManifest, Option<String>),
    /// 304 Not Modified — ETag matched, use cached data.
    NotModified,
}

/// A manifest fetch error that knows whether it should be retried.
#[derive(Debug)]
enum FetchError {
    /// Transient error (network, timeout, 5xx, 429) -- worth retrying.
    Retryable(anyhow::Error),
    /// Permanent error (404, JSON parse) -- do not retry.
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
/// - `is_request()`: request-level errors — often caused by h2 connection
///   resets or pooled connection failures, so we retry these too
/// - Everything else (e.g. `is_builder()`, `is_decode()`): permanent
fn classify_reqwest_error(e: reqwest::Error) -> FetchError {
    let is_retryable = e.is_timeout() || e.is_body() || e.is_request() || {
        #[cfg(not(target_arch = "wasm32"))]
        {
            e.is_connect()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    };
    if is_retryable {
        FetchError::Retryable(anyhow!(e))
    } else {
        FetchError::Permanent(anyhow!(e))
    }
}

/// Classify an HTTP status code as retryable or permanent.
///
/// Note: 304 Not Modified is handled separately by the caller before this
/// function is reached, so it does not appear here.
fn classify_status(status: reqwest::StatusCode, url: &str) -> FetchError {
    match status.as_u16() {
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

/// Manifest metadata format.
#[derive(Debug, Clone, Copy)]
pub enum MetadataFormat {
    /// Only install-relevant fields (deps, dist, engines, bin).
    /// Uses `application/vnd.npm.install-v1+json`, 10-50x smaller than full.
    /// Supported by all major registries.
    Abbreviated,
    /// Complete metadata including readme, time, maintainers, etc.
    /// Only needed for display commands like `utoo view`.
    Complete,
}

/// Options for fetching a full manifest.
pub struct FetchManifestOptions<'a> {
    pub registry_url: &'a str,
    pub name: &'a str,
    pub format: MetadataFormat,
    pub etag: Option<&'a str>,
}

/// Fetch full manifest with retry and ETag support.
pub async fn fetch_full_manifest(opts: FetchManifestOptions<'_>) -> Result<FetchManifestResult> {
    let url = format!("{}/{}", opts.registry_url, opts.name);
    let etag_owned = opts.etag.map(|s| s.to_string());
    let accept = match opts.format {
        MetadataFormat::Abbreviated => "application/vnd.npm.install-v1+json",
        MetadataFormat::Complete => "application/json",
    };

    tracing::debug!("Fetching full manifest for {} from {}", opts.name, url);

    RetryIf::spawn(
        retry_strategy(),
        || {
            let url = url.clone();
            let etag = etag_owned.clone();
            async move {
                let mut request = get_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept);
                if let Some(etag_value) = &etag {
                    request = request.header("If-None-Match", etag_value);
                }

                let response = request.send().await.map_err(classify_reqwest_error)?;
                let status = response.status();

                if status == reqwest::StatusCode::NOT_MODIFIED {
                    if etag.is_some() {
                        return Ok(FetchManifestResult::NotModified);
                    }
                    // Server bug: 304 without If-None-Match. Treat as error.
                    return Err(classify_status(status, &url));
                }

                if status.is_success() {
                    let new_etag = response
                        .headers()
                        .get("etag")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());

                    let raw_bytes = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .to_vec();
                    // Save raw bytes before simd_json mutates the parse buffer
                    let mut parse_buf = raw_bytes.clone();
                    let mut manifest: FullManifest =
                        simd_json::serde::from_slice(&mut parse_buf)
                            .map_err(|e| FetchError::Permanent(anyhow!("JSON parse error: {e}")))?;
                    manifest.raw = std::sync::Arc::from(raw_bytes);

                    Ok(FetchManifestResult::Ok(manifest, new_etag))
                } else {
                    Err(classify_status(status, &url))
                }
            }
        },
        is_retryable,
    )
    .await
    .map_err(|e| match e {
        FetchError::Retryable(e) | FetchError::Permanent(e) => {
            anyhow!("Failed to fetch {}: {}", opts.name, e)
        }
    })
}

/// Fetch full manifest without ETag / 304 support.
///
/// Convenience wrapper around [`fetch_full_manifest`] for callers that never
/// send `If-None-Match` (e.g. `utoo view`, corrupted-cache fallback).
/// Returns the manifest directly — no [`FetchManifestResult::NotModified`]
/// to handle.
pub async fn fetch_full_manifest_fresh(
    registry_url: &str,
    name: &str,
    format: MetadataFormat,
) -> Result<(FullManifest, Option<String>)> {
    match fetch_full_manifest(FetchManifestOptions {
        registry_url,
        name,
        format,
        etag: None,
    })
    .await?
    {
        FetchManifestResult::Ok(manifest, etag) => Ok((manifest, etag)),
        FetchManifestResult::NotModified => {
            // fetch_full_manifest with etag: None treats server 304 as an error,
            // so this variant is structurally unreachable.
            Err(anyhow!("unexpected 304 without If-None-Match for {name}"))
        }
    }
}

/// Options for fetching a version manifest.
pub struct FetchVersionManifestOptions<'a> {
    pub registry_url: &'a str,
    pub name: &'a str,
    pub spec: &'a str,
    pub format: MetadataFormat,
}

/// Fetch version manifest with retry.
pub async fn fetch_version_manifest(
    opts: FetchVersionManifestOptions<'_>,
) -> Result<CoreVersionManifest> {
    let url = format!("{}/{}/{}", opts.registry_url, opts.name, opts.spec);

    let accept = match opts.format {
        MetadataFormat::Abbreviated => "application/vnd.npm.install-v1+json",
        MetadataFormat::Complete => "application/json",
    };

    tracing::debug!(
        "Fetching version manifest for {}@{} from {}",
        opts.name,
        opts.spec,
        url
    );

    RetryIf::spawn(
        retry_strategy(),
        || {
            let url = url.clone();
            async move {
                let response = get_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept)
                    .send()
                    .await
                    .map_err(classify_reqwest_error)?;

                if response.status().is_success() {
                    let mut bytes = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .to_vec();
                    simd_json::serde::from_slice(&mut bytes)
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
            anyhow!("Failed to fetch {}@{}: {}", opts.name, opts.spec, e)
        }
    })
}
