//! Shared HTTP retry infrastructure.
//!
//! All of ruborist's fetchers share the same classification of transient vs.
//! permanent failures and the same fixed-backoff retry strategy.
//!
//! Callers map their success path into an async closure and combine
//! [`classify_reqwest_error`] + [`classify_status`] to produce a
//! [`FetchError`] that the retry layer then dispatches via [`is_retryable`].
//!
//! Consumers:
//!   `service::manifest`        — npm registry JSON (full + version manifests)
//!   `resolver::http`           — HTTP tarball byte downloads
//!
//! Note: pm has its own [`util::retry`] for tarball/binary file downloads
//! (longer backoffs, string-payload errors). The two layers are intentionally
//! separate — pm's retry doesn't run in WASM, and ruborist's `fetch` is
//! WASM-compatible because `utoo-wasm` consumes `service::build_deps`.
//!
//! [`util::retry`]: https://github.com/utooland/utoo/blob/main/crates/pm/src/util/retry.rs

use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio_retry::RetryIf;

/// HTTP response metadata preserved through the retry and registry layers.
#[derive(Debug)]
pub struct HttpStatusError {
    status: u16,
    url: String,
}

impl HttpStatusError {
    pub const fn status(&self) -> u16 {
        self.status
    }
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.url)
    }
}

impl std::error::Error for HttpStatusError {}

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
    /// Transient error (network, timeout, 5xx, 429, npmjs's flaky 406) — worth retrying.
    Retryable(anyhow::Error),
    /// Permanent error (404, parse, other 4xx) — do not retry.
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

/// Run `attempt` under the shared retry policy, mapping a terminal
/// [`FetchError`] into an `anyhow` error labelled `Failed to fetch {target}`.
///
/// This is the single envelope for registry manifest fetches — keeping the
/// retry strategy, retry predicate, and terminal error shape in one place so
/// they can't drift between the full-manifest and version-manifest fetchers.
pub(crate) async fn with_fetch_retry<T, A, Fut>(target: impl Display, attempt: A) -> Result<T>
where
    A: FnMut() -> Fut,
    Fut: Future<Output = Result<T, FetchError>>,
{
    let context = format!("Failed to fetch {target}");
    RetryIf::spawn(retry_strategy(), attempt, is_retryable)
        .await
        .map_err(|e| match e {
            FetchError::Retryable(e) | FetchError::Permanent(e) => e.context(context),
        })
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
    // Status + URL are carried by the anyhow chain; the tokio-retry caller
    // surfaces them to the user on final failure. Skip per-attempt tracing
    // here so a 10-retry loop doesn't spam the same line ten times.
    let status = status.as_u16();
    let error = || {
        anyhow!(HttpStatusError {
            status,
            url: url.to_string(),
        })
    };
    match status {
        404 => FetchError::Permanent(error()),
        // npmjs's Cloudflare edge intermittently answers a manifest request
        // with 406 under heavy concurrent fan-out (different package each time,
        // ~one per run) — a transient content-negotiation hiccup, not a real
        // "your Accept header is unsatisfiable" (that would 406 every request).
        // Retry it like a 429 rather than aborting the whole resolve.
        406 | 429 => FetchError::Retryable(error()),
        s if (500..600).contains(&s) => FetchError::Retryable(error()),
        _ => FetchError::Permanent(error()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retry_context_preserves_http_status() {
        let error = with_fetch_retry("fixture", || async {
            Err::<(), _>(classify_status(
                reqwest::StatusCode::NOT_FOUND,
                "https://registry.test/fixture",
            ))
        })
        .await
        .unwrap_err();

        let status = error
            .chain()
            .find_map(|source| source.downcast_ref::<HttpStatusError>())
            .map(HttpStatusError::status);
        assert_eq!(status, Some(404));
        assert_eq!(
            format!("{error:#}"),
            "Failed to fetch fixture: HTTP 404: https://registry.test/fixture"
        );
    }
}
