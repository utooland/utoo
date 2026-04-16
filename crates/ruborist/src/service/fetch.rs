//! Shared HTTP retry infrastructure.
//!
//! All of ruborist's fetchers (manifest JSON, http tarball downloads) share
//! the same classification of transient vs. permanent failures and the same
//! fixed-backoff retry strategy. This module centralizes both.
//!
//! Callers map their success path into an async closure and combine
//! [`classify_reqwest_error`] + [`classify_status`] to produce a
//! [`FetchError`] that the retry layer then dispatches via [`is_retryable`].

use std::time::Duration;

use anyhow::anyhow;

/// Fixed retry delays used for all fetch operations in this crate.
pub const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

/// Iterator form of [`RETRY_DELAYS`] for feeding `tokio_retry::RetryIf`.
pub fn retry_strategy() -> impl Iterator<Item = Duration> + Clone {
    RETRY_DELAYS.into_iter()
}

/// A fetch error that knows whether it should be retried.
#[derive(Debug)]
pub enum FetchError {
    /// Transient error (network, timeout, 5xx, 429) — worth retrying.
    Retryable(anyhow::Error),
    /// Permanent error (404, parse, 4xx) — do not retry.
    Permanent(anyhow::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Retryable(e) | FetchError::Permanent(e) => e.fmt(f),
        }
    }
}

/// Predicate for `RetryIf::spawn` — returns true for transient failures.
pub fn is_retryable(err: &FetchError) -> bool {
    matches!(err, FetchError::Retryable(_))
}

/// Classify a `reqwest::Error` as retryable or permanent.
///
/// - `is_timeout()` / `is_connect()`: transient network issues
/// - `is_body()`: stream read errors (e.g. connection reset mid-transfer)
/// - `is_request()`: request-level errors — often caused by h2 connection
///   resets or pooled connection failures, so we retry these too
/// - Everything else (e.g. `is_builder()`, `is_decode()`): permanent
pub fn classify_reqwest_error(e: reqwest::Error) -> FetchError {
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
/// Callers should handle the 2xx (and 304, for ETag-aware endpoints) paths
/// before reaching this function.
pub fn classify_status(status: reqwest::StatusCode, url: &str) -> FetchError {
    match status.as_u16() {
        404 => FetchError::Permanent(anyhow!("HTTP 404: {url}")),
        429 => {
            tracing::warn!("HTTP 429 (rate limited) for {}", url);
            FetchError::Retryable(anyhow!("HTTP 429: {url}"))
        }
        s if (500..600).contains(&s) => {
            tracing::warn!("HTTP {} for {}", status, url);
            FetchError::Retryable(anyhow!("HTTP {status}: {url}"))
        }
        _ => {
            tracing::warn!("HTTP {} for {}", status, url);
            FetchError::Permanent(anyhow!("HTTP {status}: {url}"))
        }
    }
}
