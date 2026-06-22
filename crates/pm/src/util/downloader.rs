use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use tokio_retry::RetryIf;
use utoo_ruborist::spec::Protocol;

use super::install_progress::{DOWNLOADED_BYTES, DownloadGuard};
use super::retry::{RetryableError, build_download_client, create_retry_strategy};

// Global downloader client. Concurrency and duplicate work are controlled by
// the caller's scheduler. Stores `Result` so proxy-configuration errors
// surface to callers instead of panicking.
static DOWNLOADER_CLIENT: Lazy<Result<Client, String>> =
    Lazy::new(|| build_download_client().map_err(|e| e.to_string()));

fn downloader_client() -> Result<&'static Client> {
    DOWNLOADER_CLIENT
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Check whether a tarball URL refers to a git-resolved package.
pub fn is_git_url(url: &str) -> bool {
    matches!(url.parse::<Protocol>(), Ok(Protocol::Git))
}

/// Download tarball bytes with retries (network phase only).
///
/// `auth_token` is attached as a Bearer header when present; callers are
/// responsible for the leak guard (only pass a token for registry-host URLs —
/// see [`crate::service::auth::token_for_url`]).
pub async fn download_bytes(url: &str, auth_token: Option<&str>) -> Result<Bytes> {
    let client = downloader_client()?;
    let retry_count = AtomicU32::new(0);
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let attempt = retry_count.fetch_add(1, Ordering::Relaxed);

            let mut request = client.get(url);
            if let Some(token) = auth_token {
                request = request.bearer_auth(token);
            }
            let mut response = request
                .send()
                .await
                .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    // Read the body chunk-by-chunk via `Response::chunk` (which
                    // needs no reqwest `stream` feature) instead of buffering it
                    // in one await: each chunk feeds the live byte counter the
                    // spinner renders as `↓ 23.4 MB 8.2 MB/s`. The body is still
                    // fully buffered before return — the downstream extractor is
                    // not streaming — so `chunk` only buys the progress signal,
                    // not lower peak memory. The guard surfaces this request in
                    // the `N downloading` concurrency count.
                    let _gauge = DownloadGuard::enter();
                    // Capacity hint only — capped so a bogus Content-Length
                    // can't force a huge allocation; BytesMut grows as needed.
                    const MAX_PREALLOC: u64 = 32 * 1024 * 1024;
                    let hint = response.content_length().unwrap_or(0).min(MAX_PREALLOC);
                    let mut buf = BytesMut::with_capacity(hint as usize);
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|e| RetryableError::Temporary(format!("Stream error: {e}")))?
                    {
                        DOWNLOADED_BYTES.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                        buf.extend_from_slice(&chunk);
                    }
                    if attempt > 0 {
                        // Debug, not info: a succeeded retry is normal recovery,
                        // not worth a console line that interrupts the spinner.
                        tracing::debug!("Retry succeeded on attempt {}: {url}", attempt + 1);
                    }
                    Ok(buf.freeze())
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
