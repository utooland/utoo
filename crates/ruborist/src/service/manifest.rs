//! Manifest fetching with retry for registry operations.
//!
//! Uses the shared classification + backoff machinery from
//! [`crate::service::fetch`] so retry policy stays uniform across registry
//! manifest fetches and non-registry resolvers (git, http tarball).

use anyhow::{Result, anyhow};
use bytes::Bytes;
use tokio_retry::RetryIf;

use super::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use super::http::get_client;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::resolver::version::resolve_target_version;

/// Parse a JSON buffer on rayon's CPU thread pool (native) or inline
/// (wasm32). The buffer is consumed because `simd_json` mutates it in-place.
/// Keeps the tokio runtime free of `simd_json` work so other in-flight
/// manifest fetches keep driving network IO while this one parses.
pub(crate) async fn parse_json_vec_off_runtime<T>(
    mut parse_buf: Vec<u8>,
) -> Result<T, anyhow::Error>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        rayon::spawn(move || {
            let result = simd_json::serde::from_slice::<T>(&mut parse_buf)
                .map_err(|e| anyhow!("JSON parse error: {e}"));
            let _ = tx.send(result);
        });
        rx.await
            .map_err(|e| anyhow!("rayon parse channel closed: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        simd_json::serde::from_slice::<T>(&mut parse_buf)
            .map_err(|e| anyhow!("JSON parse error: {e}"))
    }
}

/// Parse a full wire-fetched manifest and restore its raw byte payload.
///
/// Intended for the BFS resolver loop follow-up: fetch tasks can return bytes
/// while the loop owns cache/waiter/inflight state and chooses when to parse.
pub(crate) async fn parse_full_manifest_off_runtime(
    raw_bytes: Bytes,
) -> Result<FullManifest, anyhow::Error> {
    parse_full_manifest_with_core_off_runtime(raw_bytes, None)
        .await
        .map(|(manifest, _)| manifest)
}

pub(crate) type FullManifestParseResult = (FullManifest, Option<(String, CoreVersionManifest)>);

fn parse_full_manifest_with_core_sync(
    raw_bytes: Bytes,
    spec: Option<String>,
) -> Result<FullManifestParseResult, anyhow::Error> {
    // simd_json mutates the parse buffer; copy inside the worker so
    // response-body bytes can stay immutable until parsing starts.
    let mut parse_buf = raw_bytes.to_vec();
    let mut manifest: FullManifest = simd_json::serde::from_slice::<FullManifest>(&mut parse_buf)
        .map_err(|e| anyhow!("JSON parse error: {e}"))?;
    manifest.raw = raw_bytes;

    let speculative = spec.and_then(|spec| {
        resolve_target_version((&manifest).into(), &spec)
            .ok()
            .and_then(|version| manifest.get_core_version(&version).map(|core| (spec, core)))
    });

    Ok((manifest, speculative))
}

/// Parse a full wire-fetched manifest and optionally extract the current BFS
/// edge's version in the same off-runtime worker task.
pub(crate) async fn parse_full_manifest_with_core_off_runtime(
    raw_bytes: Bytes,
    spec: Option<String>,
) -> Result<FullManifestParseResult, anyhow::Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        rayon::spawn(move || {
            let result = parse_full_manifest_with_core_sync(raw_bytes, spec);
            let _ = tx.send(result);
        });
        rx.await
            .map_err(|e| anyhow!("rayon parse channel closed: {e}"))?
    }
    #[cfg(target_arch = "wasm32")]
    {
        parse_full_manifest_with_core_sync(raw_bytes, spec)
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
    Ok(Bytes, Option<String>),
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

                    let raw_bytes = response.bytes().await.map_err(classify_reqwest_error)?;

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

#[cfg(not(target_arch = "wasm32"))]
async fn read_body_vec(mut response: reqwest::Response) -> Result<Vec<u8>, FetchError> {
    let capacity = response
        .content_length()
        .and_then(|len| usize::try_from(len).ok())
        .unwrap_or(0);
    let mut body = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(classify_reqwest_error)? {
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(target_arch = "wasm32")]
async fn read_body_vec(response: reqwest::Response) -> Result<Vec<u8>, FetchError> {
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(classify_reqwest_error)
}

/// Options for fetching a version manifest.
pub struct FetchVersionManifestOptions<'a> {
    pub registry_url: &'a str,
    pub name: &'a str,
    pub spec: &'a str,
    pub format: MetadataFormat,
}

/// Fetch version manifest bytes with retry, without parsing.
pub async fn fetch_version_manifest_bytes(opts: FetchVersionManifestOptions<'_>) -> Result<Bytes> {
    fetch_version_manifest_vec(opts).await.map(Bytes::from)
}

/// Fetch version manifest into a mutable parse buffer with retry.
///
/// Unlike full manifests, exact-version manifests do not need to keep raw
/// response bytes for later extraction. Reading directly into `Vec<u8>` avoids
/// the hot-path `Bytes -> Vec` copy before `simd_json` parses in place.
pub(crate) async fn fetch_version_manifest_vec(
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
                    read_body_vec(response).await
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
    let bytes = fetch_version_manifest_vec(opts).await?;
    parse_json_vec_off_runtime::<CoreVersionManifest>(bytes).await
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TinyManifest {
        name: String,
        version: String,
    }

    #[tokio::test]
    async fn parse_json_vec_off_runtime_consumes_mutable_buffer() {
        let parsed = parse_json_vec_off_runtime::<TinyManifest>(
            br#"{"name":"demo","version":"1.0.0"}"#.to_vec(),
        )
        .await
        .unwrap();

        assert_eq!(
            parsed,
            TinyManifest {
                name: "demo".to_string(),
                version: "1.0.0".to_string(),
            }
        );
    }
}
