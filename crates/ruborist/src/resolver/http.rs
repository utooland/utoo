//! HTTP(S) tarball resolver for `https://…/pkg.tgz` dependency specs.
//!
//! # Why BFS only reads the manifest (no global cache)
//!
//! A non-registry remote tarball is indistinguishable from a registry
//! download URL in an npm `package-lock.json` — both are `https://….tgz`,
//! and the lockfile stores no source tag. Routing such a tarball into the
//! shared `<name>/<version>` global cache would let an attacker-controlled
//! URL self-declare any `name@version` and poison the slot the registry
//! uses for a real package (there is no `dist.integrity` to verify against
//! on a cache hit).
//!
//! So non-registry tarballs never enter the global cache. BFS downloads the
//! tarball only to parse its manifest (name/version/deps + the pinned
//! `dist.tarball`) so transitive resolution can continue; the bytes are
//! dropped. At install time pm re-fetches the URL and extracts the contents
//! **directly** into `node_modules/<pkg>` (see
//! [`crate::tar::extract_tarball_to_dir`]). HTTP tarballs are therefore
//! downloaded twice (manifest in BFS, content at install) — accepted, as
//! remote tarballs are rare and this removes the cache-poisoning and
//! path-hashed-slot-staleness problems entirely.
//!
//! # Flow
//!
//! ```text
//!   package.json:  "foo": "https://example.com/foo-1.2.3.tgz"
//!                                    │
//!  ┌── BFS resolution (this module) ──────────────────────────────────────────┐
//!  │                                 ▼                                        │
//!  │  resolve_http_dep(url, &fetch_cache)                                     │
//!  │       │                                                                  │
//!  │       ▼                                                                  │
//!  │  HttpFetchCache  ── dedup_init  ★  (one fetch per URL across BFS)        │
//!  │       │                                                                  │
//!  │       ▼                                                                  │
//!  │  download_tarball  ── FetchError + classify_*  ☆                         │
//!  │       │  Bytes                                                           │
//!  │       ▼                                                                  │
//!  │  parse_tarball_manifest (no extraction; finalize_non_registry_manifest)  │
//!  │       │                                                                  │
//!  │       ▼                                                                  │
//!  │  ResolvedPackage { dist.tarball = url, … }    ── bytes dropped here      │
//!  └───────│──────────────────────────────────────────────────────────────────┘
//!          ▼  (lockfile)
//!  ┌── Install phase (pm) ─────────────────────────────────────────────────────┐
//!  │  non-registry tarball → re-download URL → extract_tarball_to_dir into     │
//!  │  node_modules/<pkg> (no global cache slot)                                │
//!  └──────────────────────────────────────────────────────────────────────────┘
//!
//!  Legend:
//!    ★ shared with the git resolver via `super::common`
//!      (DedupCache, dedup_init, finalize_non_registry_manifest,
//!       validate_package_name)
//!    ☆ shared with registry manifest fetching via `crate::service::fetch`
//!      (FetchError, classify_reqwest_error, classify_status, retry_strategy)
//! ```

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use tokio_retry::RetryIf;

use super::common::{DedupCache, dedup_init};
use super::tar::parse_tarball_manifest;
use crate::model::manifest::CoreVersionManifest;
use crate::service::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use crate::service::http::get_client;
use crate::traits::registry::ResolvedPackage;

/// Session-scoped dedup cache: one fetch per URL even under concurrent BFS.
pub(crate) type HttpFetchCache = DedupCache<CoreVersionManifest>;

// ============================================================================
// Async download + retry
// ============================================================================

async fn download_tarball(url: &str) -> Result<Bytes> {
    RetryIf::spawn(
        retry_strategy(),
        || {
            let url = url.to_string();
            async move {
                let resp = get_client()
                    .map_err(FetchError::Permanent)?
                    .get(&url)
                    .send()
                    .await
                    .map_err(classify_reqwest_error)?;
                let status = resp.status();
                if !status.is_success() {
                    return Err(classify_status(status, &url));
                }
                resp.bytes()
                    .await
                    .map_err(|e| FetchError::Retryable(anyhow!("stream error: {e}")))
            }
        },
        is_retryable,
    )
    .await
    .map_err(|e| match e {
        FetchError::Retryable(e) | FetchError::Permanent(e) => e,
    })
    .with_context(|| format!("failed to fetch tarball from {url}"))
}

// ============================================================================
// High-level resolver — called by BFS `process_dependency`
// ============================================================================

/// Resolve an HTTP tarball spec to a [`ResolvedPackage`].
///
/// Downloads the tarball only to parse its manifest; the bytes are not cached.
/// The tarball content is materialized directly into `node_modules` at install
/// time, so non-registry tarballs never enter the global cache.
pub(crate) async fn resolve_http_dep(
    url: &str,
    fetch_cache: &HttpFetchCache,
) -> Result<ResolvedPackage> {
    let url_owned = url.to_string();
    let manifest = dedup_init(fetch_cache, url_owned.clone(), move || async move {
        let bytes = download_tarball(&url_owned).await?;
        let url_for_blocking = url_owned.clone();
        tokio::task::spawn_blocking(move || parse_tarball_manifest(&bytes, url_for_blocking))
            .await
            .context("http tarball manifest parse task failed")?
    })
    .await?;

    Ok(ResolvedPackage::from_manifest(
        manifest.name.clone(),
        manifest,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_targz(entries: &[(&str, &[u8])]) -> Bytes {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            for (path, body) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append(&header, *body).unwrap();
            }
            tar.finish().unwrap();
        }
        let mut compressor = libdeflater::Compressor::new(libdeflater::CompressionLvl::default());
        let mut compressed = vec![0u8; compressor.gzip_compress_bound(tar_data.len())];
        let n = compressor
            .gzip_compress(&tar_data, &mut compressed)
            .unwrap();
        compressed.truncate(n);
        Bytes::from(compressed)
    }

    #[test]
    fn parses_manifest_with_pinned_url() {
        let pkg = br#"{"name":"demo","version":"1.2.3","scripts":{"install":"echo hi"}}"#;
        let bytes = make_targz(&[("package/package.json", pkg)]);
        let url = "https://example.com/demo.tgz";

        let manifest = parse_tarball_manifest(&bytes, url.to_string()).unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.has_install_script, Some(true));
        assert_eq!(manifest.dist.tarball.as_deref(), Some(url));
    }

    #[test]
    fn rejects_tarball_without_package_json() {
        let bytes = make_targz(&[("package/README.md", b"hi")]);
        let err = parse_tarball_manifest(&bytes, "u".to_string()).unwrap_err();
        assert!(err.to_string().contains("package.json not found"));
    }

    #[test]
    fn shallowest_package_json_wins() {
        let bytes = make_targz(&[
            (
                "package/sub/package.json",
                br#"{"name":"nested","version":"9.9.9"}"#,
            ),
            (
                "package/package.json",
                br#"{"name":"demo","version":"1.0.0"}"#,
            ),
        ]);
        let manifest = parse_tarball_manifest(&bytes, "u".to_string()).unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.0.0");
    }
}
