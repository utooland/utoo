//! Manifest fetching with retry for registry operations.
//!
//! Uses the shared classification + backoff machinery from
//! [`crate::service::fetch`] so retry policy stays uniform across registry
//! manifest fetches and non-registry resolvers (git, http tarball).

use anyhow::{Result, anyhow};
use tokio_retry::RetryIf;

use super::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use super::http::get_client;
use crate::model::manifest::{CoreVersionManifest, FullManifest};

/// Collect per-request timing samples for percentile reporting.
/// Prints a histogram every [`HISTO_REPORT_EVERY`] calls so the final
/// output reflects the full distribution of the run.
fn record_sample(send_us: u32, body_us: u32, bytes: u32) {
    use std::sync::Mutex;
    use std::sync::OnceLock;
    struct Samples {
        send: Vec<u32>,
        body: Vec<u32>,
        bytes: Vec<u32>,
    }
    static STORE: OnceLock<Mutex<Samples>> = OnceLock::new();
    const HISTO_REPORT_EVERY: usize = 500;
    let store = STORE.get_or_init(|| {
        Mutex::new(Samples {
            send: Vec::new(),
            body: Vec::new(),
            bytes: Vec::new(),
        })
    });
    let mut s = store.lock().unwrap();
    s.send.push(send_us);
    s.body.push(body_us);
    s.bytes.push(bytes);
    let n = s.send.len();
    if n % HISTO_REPORT_EVERY == 0 || n == 1 {
        fn pct(v: &mut [u32], p: f64) -> u32 {
            if v.is_empty() {
                return 0;
            }
            v.sort_unstable();
            let idx = ((p * v.len() as f64) as usize).min(v.len() - 1);
            v[idx]
        }
        let mut send = s.send.clone();
        let mut body = s.body.clone();
        let mut bytes = s.bytes.clone();
        eprintln!(
            "  [histo #{}] send p50={}ms p90={}ms p99={}ms max={}ms | body p50={}ms p90={}ms p99={}ms max={}ms | bytes p50={}KB p90={}KB max={}KB",
            n,
            pct(&mut send, 0.50) / 1000,
            pct(&mut send, 0.90) / 1000,
            pct(&mut send, 0.99) / 1000,
            pct(&mut send, 1.0) / 1000,
            pct(&mut body, 0.50) / 1000,
            pct(&mut body, 0.90) / 1000,
            pct(&mut body, 0.99) / 1000,
            pct(&mut body, 1.0) / 1000,
            pct(&mut bytes, 0.50) / 1024,
            pct(&mut bytes, 0.90) / 1024,
            pct(&mut bytes, 1.0) / 1024,
        );
    }
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

                use std::time::Instant;
                let t_send = Instant::now();
                let response = request.send().await.map_err(classify_reqwest_error)?;
                let send_us = t_send.elapsed().as_micros() as u32;
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

                    let t_body = Instant::now();
                    let raw_bytes = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .to_vec();
                    let body_us = t_body.elapsed().as_micros() as u32;
                    record_sample(send_us, body_us, raw_bytes.len() as u32);

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
            anyhow!("Failed to fetch {}: {:#}", opts.name, e)
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
            anyhow!("Failed to fetch {}@{}: {:#}", opts.name, opts.spec, e)
        }
    })
}
