//! Manifest fetching with retry for registry operations.
//!
//! Uses the shared classification + backoff machinery from
//! [`crate::service::fetch`] so retry policy stays uniform across registry
//! manifest fetches and non-registry resolvers (git, http tarball).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use serde::Deserialize;
use tokio_retry::RetryIf;

use super::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use super::http::get_client;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::resolver::version::resolve_target_version;
use crate::util::FETCH_TIMINGS;

/// Parse JSON bytes on tokio's blocking thread pool.
///
/// The history of this function captures three different attempts:
///   - rayon::spawn (original): rayon's pool is `num_cpus` (= 2 on
///     GHA), 64 concurrent parses queued behind 2 workers → avg_parse
///     30ms wall vs ~5ms CPU. round-0 baseline.
///   - inline (round 1, reverted): no rayon hop, but the simd_json
///     call blocks the tokio runtime worker, so other in-flight
///     fetches couldn't drive their socket I/O — avg_request grew
///     35ms → 52ms (+17ms), eff_parallel 42 → 35, net p1 wall +0.37s.
///   - spawn_blocking (current): tokio's dedicated blocking pool has
///     a much higher default cap (512), so 64 concurrent parses are
///     never queued. Unlike rayon there's no contention with the
///     install path's parallel-write rayon usage, and unlike inline
///     the tokio runtime workers stay free to drive network I/O on
///     all in-flight fetches.
async fn parse_json_off_runtime<T>(mut bytes: Vec<u8>) -> Result<T, anyhow::Error>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(move || {
            simd_json::serde::from_slice::<T>(&mut bytes)
                .map_err(|e| anyhow!("JSON parse error: {e}"))
        })
        .await
        .map_err(|e| anyhow!("spawn_blocking parse panicked: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        simd_json::serde::from_slice::<T>(&mut bytes).map_err(|e| anyhow!("JSON parse error: {e}"))
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

                let t_request_start = std::time::Instant::now();
                let response = request.send().await.map_err(classify_reqwest_error)?;
                let request_us = t_request_start.elapsed().as_micros() as u64;
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

                    let t_body_start = std::time::Instant::now();
                    let raw_bytes = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .to_vec();
                    let body_us = t_body_start.elapsed().as_micros() as u64;
                    let bytes_len = raw_bytes.len() as u64;
                    // simd_json mutates the parse buffer; clone so the raw
                    // bytes survive for `manifest.raw`.
                    let parse_buf = raw_bytes.clone();
                    let t_parse_start = std::time::Instant::now();
                    let mut manifest: FullManifest = parse_json_off_runtime(parse_buf)
                        .await
                        .map_err(FetchError::Permanent)?;
                    let parse_us = t_parse_start.elapsed().as_micros() as u64;
                    manifest.raw = std::sync::Arc::from(raw_bytes);

                    FETCH_TIMINGS.record(request_us, body_us, parse_us, bytes_len);
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

/// Outcome of [`fetch_full_manifest_with_settle`] — a full manifest
/// plus the parsed `CoreVersionManifest` for the requested spec, when
/// it resolves to a known version. Both are produced from a single
/// `simd_json::to_borrowed_value` pass over the response body, so
/// callers that need the version subtree never pay the typed-serde
/// envelope parse + per-version `to_borrowed_value` reparse.
pub struct FetchWithSettle {
    pub manifest: FullManifest,
    pub etag: Option<String>,
    /// `Some` when the requested spec resolves to a real version in
    /// `manifest.versions`. `None` only on no-match (rare; usually a
    /// spec referring to a yanked or moved version).
    pub primary_settle: Option<PrimarySettleResult>,
}

/// `(resolved_version, parsed_subtree)` — what
/// [`fetch_full_manifest_with_settle`] hands back to callers that
/// supplied a `primary_spec`.
pub type PrimarySettleResult = (String, Arc<CoreVersionManifest>);

#[allow(clippy::large_enum_variant)]
pub enum FetchWithSettleResult {
    Ok(FetchWithSettle),
    NotModified,
}

/// Fetch a full manifest and resolve the primary spec from the same
/// parse pass.
///
/// Where [`fetch_full_manifest`] uses `simd_json::serde::from_slice`
/// to materialize a typed `FullManifest` (cheap envelope, deep
/// `versions` subtrees skipped via `IgnoredAny`) and leaves version
/// subtree extraction to a later `simd_json::to_borrowed_value`
/// reparse, this entry point does the borrowed-value parse once and
/// extracts:
///   * envelope fields needed by the resolver (`name`, `dist-tags`,
///     `versions` keys),
///   * the resolved-version subtree as a typed
///     [`CoreVersionManifest`].
///
/// Saves one full simd_json pass on the parse hot path —
/// `fast_preload` uses ~2700 of these per `utoo deps` cold run, so
/// halving the per-fetch parse work meaningfully reduces CPU on
/// 2-core CI.
pub async fn fetch_full_manifest_with_settle(
    opts: FetchManifestOptions<'_>,
    primary_spec: &str,
) -> Result<FetchWithSettleResult> {
    let url = format!("{}/{}", opts.registry_url, opts.name);
    let etag_owned = opts.etag.map(|s| s.to_string());
    let primary_spec_owned = primary_spec.to_string();
    let accept = match opts.format {
        MetadataFormat::Abbreviated => "application/vnd.npm.install-v1+json",
        MetadataFormat::Complete => "application/json",
    };

    RetryIf::spawn(
        retry_strategy(),
        || {
            let url = url.clone();
            let etag = etag_owned.clone();
            let primary_spec = primary_spec_owned.clone();
            async move {
                let mut request = get_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept);
                if let Some(etag_value) = &etag {
                    request = request.header("If-None-Match", etag_value);
                }

                let t_request_start = std::time::Instant::now();
                let response = request.send().await.map_err(classify_reqwest_error)?;
                let request_us = t_request_start.elapsed().as_micros() as u64;
                let status = response.status();

                if status == reqwest::StatusCode::NOT_MODIFIED {
                    if etag.is_some() {
                        return Ok(FetchWithSettleResult::NotModified);
                    }
                    return Err(classify_status(status, &url));
                }

                if status.is_success() {
                    let new_etag = response
                        .headers()
                        .get("etag")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());

                    let t_body_start = std::time::Instant::now();
                    let raw_bytes = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .to_vec();
                    let body_us = t_body_start.elapsed().as_micros() as u64;
                    let bytes_len = raw_bytes.len() as u64;
                    let raw_arc: Arc<[u8]> = Arc::from(raw_bytes);

                    let t_parse_start = std::time::Instant::now();
                    let parse_result =
                        parse_envelope_and_settle(Arc::clone(&raw_arc), primary_spec)
                            .await
                            .map_err(FetchError::Permanent)?;
                    let parse_us = t_parse_start.elapsed().as_micros() as u64;

                    FETCH_TIMINGS.record(request_us, body_us, parse_us, bytes_len);

                    let (manifest, primary_settle) = parse_result;
                    Ok(FetchWithSettleResult::Ok(FetchWithSettle {
                        manifest,
                        etag: new_etag,
                        primary_settle,
                    }))
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

/// Off-runtime combined parse: `simd_json::to_borrowed_value` once,
/// extract envelope into [`FullManifest`] + resolve `primary_spec`
/// against the parsed `versions` keys + materialize the resolved
/// version's subtree into [`CoreVersionManifest`].
///
/// Constructs `FullManifest` manually rather than via typed serde so
/// the work is exactly one parse pass. Other `FullManifest` fields
/// (`description`, `time`, `maintainers`, etc.) stay at `Default`
/// values — none are read on the resolver hot path.
async fn parse_envelope_and_settle(
    raw: Arc<[u8]>,
    primary_spec: String,
) -> Result<(FullManifest, Option<PrimarySettleResult>)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(move || parse_envelope_and_settle_sync(raw, &primary_spec))
            .await
            .map_err(|e| anyhow!("spawn_blocking parse panicked: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        parse_envelope_and_settle_sync(raw, &primary_spec)
    }
}

fn parse_envelope_and_settle_sync(
    raw: Arc<[u8]>,
    primary_spec: &str,
) -> Result<(FullManifest, Option<PrimarySettleResult>)> {
    use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};

    let mut buf = (*raw).to_vec();
    let parsed =
        simd_json::to_borrowed_value(&mut buf).map_err(|e| anyhow!("JSON parse error: {e}"))?;

    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let dist_tags: HashMap<String, String> = parsed
        .get("dist-tags")
        .and_then(|v| HashMap::<String, String>::deserialize(v).ok())
        .unwrap_or_default();

    let versions_keys: Vec<String> = parsed
        .get("versions")
        .and_then(simd_json::prelude::ValueAsObject::as_object)
        .map(|obj| obj.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();

    let manifest = FullManifest {
        name,
        dist_tags: dist_tags.clone(),
        versions: versions_keys,
        raw,
        ..Default::default()
    };

    // Resolve spec against the just-extracted envelope.
    let primary_settle = match resolve_target_version((&manifest).into(), primary_spec) {
        Ok(resolved) => parsed
            .get("versions")
            .and_then(|v| v.get(resolved.as_str()))
            .and_then(|version_obj| CoreVersionManifest::deserialize(version_obj).ok())
            .map(|core| (resolved, Arc::new(core))),
        Err(_) => None,
    };

    Ok((manifest, primary_settle))
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

    RetryIf::spawn(
        retry_strategy(),
        || {
            let url = url.clone();
            async move {
                let t_request_start = std::time::Instant::now();
                let response = get_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept)
                    .send()
                    .await
                    .map_err(classify_reqwest_error)?;
                let request_us = t_request_start.elapsed().as_micros() as u64;

                if response.status().is_success() {
                    let t_body_start = std::time::Instant::now();
                    let bytes = response
                        .bytes()
                        .await
                        .map_err(|e| FetchError::Permanent(anyhow!("Response read error: {e}")))?
                        .to_vec();
                    let body_us = t_body_start.elapsed().as_micros() as u64;
                    let bytes_len = bytes.len() as u64;
                    let t_parse_start = std::time::Instant::now();
                    let parsed = parse_json_off_runtime::<CoreVersionManifest>(bytes)
                        .await
                        .map_err(FetchError::Permanent);
                    let parse_us = t_parse_start.elapsed().as_micros() as u64;
                    if parsed.is_ok() {
                        FETCH_TIMINGS.record(request_us, body_us, parse_us, bytes_len);
                    }
                    parsed
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
