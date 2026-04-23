//! Manifest fetching with retry for registry operations.
//!
//! Uses the shared classification + backoff machinery from
//! [`crate::service::fetch`] so retry policy stays uniform across registry
//! manifest fetches and non-registry resolvers (git, http tarball).

use std::sync::{LazyLock, Mutex};

use anyhow::{Result, anyhow};
use tokio_retry::RetryIf;

use super::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use super::http::pick_client;
use crate::model::manifest::{CoreVersionManifest, FullManifest};

/// Per-request send latency — from `request.send().await` entry to response
/// headers available (μs). Isolates "waiting for server to respond" from
/// body download and JSON parse.
static SEND_US: LazyLock<Mutex<Vec<u32>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Per-request body download latency — from response headers to full body
/// bytes in memory (μs). Isolates network throughput cost.
static BODY_US: LazyLock<Mutex<Vec<u32>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Per-request JSON parse latency (μs), measured end-to-end on the main
/// task: includes `spawn_blocking` dispatch + parse work + await overhead.
/// This is what the async preload loop actually waits on, so it's the right
/// slice to compare against the pure-parse cost to reveal scheduler gap.
static PARSE_US: LazyLock<Mutex<Vec<u32>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn record_us(slot: &Mutex<Vec<u32>>, us: u128) {
    if let Ok(mut v) = slot.lock() {
        v.push(us.min(u32::MAX as u128) as u32);
    }
}

/// Dump send/body/parse histograms collected during manifest fetching and
/// clear the buffers. Called from `preload_manifests` after its `proc_us`
/// dump so the three numbers are printed in one place per run.
pub fn dump_fetch_histograms() {
    for (label, slot) in [
        ("send", &*SEND_US),
        ("body", &*BODY_US),
        ("parse", &*PARSE_US),
    ] {
        let mut v = match slot.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => continue,
        };
        if v.is_empty() {
            continue;
        }
        v.sort_unstable();
        let pct = |p: f64| -> u32 {
            let idx = ((p * v.len() as f64) as usize).min(v.len() - 1);
            v[idx]
        };
        let sum: u64 = v.iter().map(|&x| x as u64).sum();
        tracing::info!(
            "manifest {}_us (n={}): p50={} p90={} p99={} max={} sum={}us avg={}us",
            label,
            v.len(),
            pct(0.50),
            pct(0.90),
            pct(0.99),
            pct(1.0),
            sum,
            sum / v.len() as u64
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
                let mut request = pick_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept);
                if let Some(etag_value) = &etag {
                    request = request.header("If-None-Match", etag_value);
                }

                let send_start = tokio::time::Instant::now();
                let response = request.send().await.map_err(classify_reqwest_error)?;
                record_us(&SEND_US, send_start.elapsed().as_micros());
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

                    let body_start = tokio::time::Instant::now();
                    let raw_bytes: Vec<u8> = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .into();
                    record_us(&BODY_US, body_start.elapsed().as_micros());

                    // Offload JSON parse to the blocking pool. Manifests are
                    // 5–50KB and simd_json is CPU-bound (~1–5ms per call);
                    // keeping this on the async main task serialises it with
                    // every other manifest response across the ~3550
                    // concurrent fetches, creating the dips we saw in the
                    // active-stream pcap. spawn_blocking lets the main task
                    // keep dispatching while the worker pool parses.
                    let parse_start = tokio::time::Instant::now();
                    let manifest = tokio::task::spawn_blocking(move || {
                        // simd_json mutates the parse buffer in place
                        // (in-place unicode unescaping etc.), so we keep a
                        // separate copy for `manifest.raw`.
                        let mut parse_buf = raw_bytes.clone();
                        let mut m: FullManifest = simd_json::serde::from_slice(&mut parse_buf)
                            .map_err(|e| anyhow!("JSON parse error: {e}"))?;
                        m.raw = std::sync::Arc::from(raw_bytes);
                        Ok::<_, anyhow::Error>(m)
                    })
                    .await
                    .map_err(|e| FetchError::Permanent(anyhow!("Parse task panicked: {e}")))?
                    .map_err(FetchError::Permanent)?;
                    record_us(&PARSE_US, parse_start.elapsed().as_micros());

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
                let response = pick_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept)
                    .send()
                    .await
                    .map_err(classify_reqwest_error)?;

                if response.status().is_success() {
                    let bytes: Vec<u8> = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .into();
                    tokio::task::spawn_blocking(move || {
                        let mut buf = bytes;
                        simd_json::serde::from_slice::<CoreVersionManifest>(&mut buf)
                            .map_err(|e| anyhow!("JSON parse error: {e}"))
                    })
                    .await
                    .map_err(|e| FetchError::Permanent(anyhow!("Parse task panicked: {e}")))?
                    .map_err(FetchError::Permanent)
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
