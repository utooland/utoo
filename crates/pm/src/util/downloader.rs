use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use tokio_retry::RetryIf;
use utoo_ruborist::spec::Protocol;

use super::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};

// Global downloader client. Concurrency and duplicate work are controlled by
// the caller's scheduler.
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

/// Check whether a tarball URL refers to a git-resolved package.
pub fn is_git_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::Git))
}

/// Check whether a tarball URL should be fetched by the registry downloader.
pub fn is_registry_tarball_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::Http))
}

/// Download tarball bytes with retries (network phase only).
///
/// `auth_token` is attached as a Bearer header when present; callers are
/// responsible for the leak guard (only pass a token for registry-host URLs —
/// see [`crate::service::auth::token_for_url`]).
pub async fn download_bytes(url: &str, auth_token: Option<&str>) -> Result<Bytes> {
    let retry_count = AtomicU32::new(0);
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let attempt = retry_count.fetch_add(1, Ordering::Relaxed);

            let mut request = DOWNLOADER_CLIENT.get(url);
            if let Some(token) = auth_token {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    let bytes = response
                        .bytes()
                        .await
                        .map_err(|e| RetryableError::Temporary(format!("Stream error: {e}")))?;
                    if attempt > 0 {
                        tracing::info!("Retry succeeded on attempt {}: {url}", attempt + 1);
                    }
                    Ok(bytes)
                }
                StatusCode::NOT_FOUND => Err(RetryableError::Permanent(format!("HTTP 404: {url}"))),
                status => Err(RetryableError::Temporary(format!("HTTP {status}: {url}"))),
            }
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await
    .with_context(|| format!("Download failed after retries: {url}"))
}
