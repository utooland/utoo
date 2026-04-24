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
use super::http::{parse_trace_enabled, pick_client, record_http_interval, record_parse_interval};
use crate::model::manifest::{CoreVersionManifest, FullManifest};

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

                let send_start = std::time::Instant::now();
                let response = request.send().await.map_err(classify_reqwest_error)?;
                let status = response.status();

                if status == reqwest::StatusCode::NOT_MODIFIED {
                    // 304 has no body but the round-trip still uses the wire;
                    // record the headers-only window so `busy` doesn't lose it.
                    record_http_interval(send_start, std::time::Instant::now());
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

                    let raw_bytes: Vec<u8> = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .into();
                    record_http_interval(send_start, std::time::Instant::now());

                    // Offload JSON parse to the blocking pool. Manifests are
                    // 5–50KB and simd_json is CPU-bound (~1–5ms per call);
                    // keeping this on the async main task serialises it with
                    // every other manifest response across the ~3550
                    // concurrent fetches, creating the dips we saw in the
                    // active-stream pcap. spawn_blocking lets the main task
                    // keep dispatching while the worker pool parses.
                    let traced = parse_trace_enabled();
                    let queued_at = if traced {
                        Some(std::time::Instant::now())
                    } else {
                        None
                    };
                    let manifest = tokio::task::spawn_blocking(move || {
                        let exec_start = queued_at.map(|_| std::time::Instant::now());
                        // simd_json mutates the parse buffer in place
                        // (in-place unicode unescaping etc.), so we keep a
                        // separate copy for `manifest.raw`.
                        let mut parse_buf = raw_bytes.clone();
                        let mut m: FullManifest = simd_json::serde::from_slice(&mut parse_buf)
                            .map_err(|e| anyhow!("JSON parse error: {e}"))?;
                        m.raw = std::sync::Arc::from(raw_bytes);
                        if let (Some(q), Some(s)) = (queued_at, exec_start) {
                            record_parse_interval(q, s, std::time::Instant::now());
                        }
                        Ok::<_, anyhow::Error>(m)
                    })
                    .await
                    .map_err(|e| FetchError::Permanent(anyhow!("Parse task panicked: {e}")))?
                    .map_err(FetchError::Permanent)?;

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
                let send_start = std::time::Instant::now();
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
                    record_http_interval(send_start, std::time::Instant::now());
                    let traced = parse_trace_enabled();
                    let queued_at = if traced {
                        Some(std::time::Instant::now())
                    } else {
                        None
                    };
                    tokio::task::spawn_blocking(move || {
                        let exec_start = queued_at.map(|_| std::time::Instant::now());
                        let mut buf = bytes;
                        let result = simd_json::serde::from_slice::<CoreVersionManifest>(&mut buf)
                            .map_err(|e| anyhow!("JSON parse error: {e}"));
                        if let (Some(q), Some(s)) = (queued_at, exec_start) {
                            record_parse_interval(q, s, std::time::Instant::now());
                        }
                        result
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
