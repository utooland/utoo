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

/// Parse JSON bytes on rayon's CPU thread pool (native) or inline
/// (wasm32). Keeps the tokio runtime free of `simd_json` work so other
/// in-flight manifest fetches keep driving network IO while this one
/// parses.
pub(crate) async fn parse_json_off_runtime<T>(mut bytes: Vec<u8>) -> Result<T, anyhow::Error>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        rayon::spawn(move || {
            let result = simd_json::serde::from_slice::<T>(&mut bytes)
                .map_err(|e| anyhow!("JSON parse error: {e}"));
            let _ = tx.send(result);
        });
        rx.await
            .map_err(|e| anyhow!("rayon parse channel closed: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        simd_json::serde::from_slice::<T>(&mut bytes).map_err(|e| anyhow!("JSON parse error: {e}"))
    }
}

/// Parse a full wire-fetched manifest and restore its raw byte payload.
///
/// Intended for the BFS resolver loop follow-up: fetch tasks can return bytes
/// while the loop owns cache/waiter/inflight state and chooses when to parse.
pub(crate) async fn parse_full_manifest_off_runtime(
    raw_bytes: Vec<u8>,
) -> Result<FullManifest, anyhow::Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        rayon::spawn(move || {
            let result = (|| -> Result<FullManifest, anyhow::Error> {
                // simd_json mutates the parse buffer; clone so the raw bytes
                // survive for `manifest.raw` and later on-demand version extraction.
                let mut parse_buf = raw_bytes.clone();
                let mut manifest: FullManifest =
                    simd_json::serde::from_slice::<FullManifest>(&mut parse_buf)
                        .map_err(|e| anyhow!("JSON parse error: {e}"))?;
                manifest.raw = std::sync::Arc::from(raw_bytes);

                Ok(manifest)
            })();
            let _ = tx.send(result);
        });
        rx.await
            .map_err(|e| anyhow!("rayon parse channel closed: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        // simd_json mutates the parse buffer; clone so the raw bytes
        // survive for `manifest.raw` and later on-demand version extraction.
        let parse_buf = raw_bytes.clone();
        let mut manifest: FullManifest = parse_json_off_runtime(parse_buf).await?;
        manifest.raw = std::sync::Arc::from(raw_bytes);
        Ok(manifest)
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

/// Raw full-manifest HTTP result.
///
/// This variant intentionally stops before JSON parsing so dependency
/// resolution loops can keep global inflight/cache ownership in one task and
/// reserve spawned work for request I/O.
pub enum FetchManifestBytesResult {
    /// 200 OK — response bytes with optional new ETag.
    Ok(Vec<u8>, Option<String>),
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

/// Fetch full manifest bytes with retry and ETag support, without parsing.
pub async fn fetch_full_manifest_bytes(
    opts: FetchManifestOptions<'_>,
) -> Result<FetchManifestBytesResult> {
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

                let response = request.send().await.map_err(classify_reqwest_error)?;
                let status = response.status();

                if status == reqwest::StatusCode::NOT_MODIFIED {
                    if etag.is_some() {
                        return Ok(FetchManifestBytesResult::NotModified);
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
                        .map_err(classify_reqwest_error)?
                        .to_vec();

                    Ok(FetchManifestBytesResult::Ok(raw_bytes, new_etag))
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

/// Fetch full manifest with retry and ETag support.
pub async fn fetch_full_manifest(opts: FetchManifestOptions<'_>) -> Result<FetchManifestResult> {
    match fetch_full_manifest_bytes(opts).await? {
        FetchManifestBytesResult::Ok(raw_bytes, etag) => {
            let manifest = parse_full_manifest_off_runtime(raw_bytes).await?;

            Ok(FetchManifestResult::Ok(manifest, etag))
        }
        FetchManifestBytesResult::NotModified => Ok(FetchManifestResult::NotModified),
    }
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

/// Fetch version manifest bytes with retry, without parsing.
pub async fn fetch_version_manifest_bytes(
    opts: FetchVersionManifestOptions<'_>,
) -> Result<Vec<u8>> {
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
                let response = get_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .header("Accept", accept)
                    .send()
                    .await
                    .map_err(classify_reqwest_error)?;

                if response.status().is_success() {
                    response
                        .bytes()
                        .await
                        .map(|b| b.to_vec())
                        .map_err(classify_reqwest_error)
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

/// Fetch version manifest with retry.
pub async fn fetch_version_manifest(
    opts: FetchVersionManifestOptions<'_>,
) -> Result<CoreVersionManifest> {
    let bytes = fetch_version_manifest_bytes(opts).await?;
    parse_json_off_runtime::<CoreVersionManifest>(bytes).await
}
