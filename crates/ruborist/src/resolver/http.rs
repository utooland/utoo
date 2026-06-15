//! HTTP(S) tarball resolver for `https://…/pkg.tgz` dependency specs.
//!
//! # Why BFS extracts up-front (like git) instead of deferring to install
//!
//! A naive "just parse the manifest in BFS, let pm re-download at install
//! time" design looks simpler but has two problems:
//!
//! 1. **Cache-slot collision**. pm's `download_to_cache` keys on
//!    `<name>/<version>/`. A URL-supplied tarball can self-declare any
//!    `name@version`, so an attacker-controlled URL can poison the same
//!    slot the npm registry uses for `lodash@4.17.21`. There is no
//!    `dist.integrity` to verify against on cache hit.
//!
//! 2. **Double download**. BFS downloads the tarball to read its manifest,
//!    then throws the bytes away; install downloads the same URL again to
//!    extract it.
//!
//! We fix both by extracting in BFS to a **URL-hashed** cache slot —
//! `<cache>/<name>/_http_<sha256(url)[:16]>/`. URL is the natural content
//! address (there is no etag/integrity to rely on, but the URL is what the
//! user committed to in package.json), so it plays the same role `<sha>`
//! plays for git. Install's `resolve_cache_path` checks this slot *before*
//! falling through to the registry cache path, so registry tarballs stay
//! unchanged while HTTP tarballs never re-download.
//!
//! Same-URL content changes **are not detected** — npm-land convention is
//! that tarball URLs are immutable. Users who break that rotate the URL or
//! run `utoo clean`.
//!
//! # Flow
//!
//! ```text
//!   package.json:  "foo": "https://example.com/foo-1.2.3.tgz"
//!                                    │
//!  ┌── BFS resolution (this module) ──────────────────────────────────────────┐
//!  │                                 ▼                                        │
//!  │  resolve_http_dep(cache_dir, url, &fetch_cache)                          │
//!  │       │                                                                  │
//!  │       ▼                                                                  │
//!  │  HttpFetchCache  ── dedup_init  ★                                        │
//!  │       │           (one fetch per URL across BFS)                         │
//!  │       ▼                                                                  │
//!  │  download_tarball  ── FetchError + classify_*  ☆                         │
//!  │       │                                                                  │
//!  │       ▼  Bytes                                                           │
//!  │  spawn_blocking → fetch_and_extract_blocking:                            │
//!  │       gzip_decompress (libdeflater)                                      │
//!  │       scan_tarball  (single pass; collects entries + finds pkg.json)     │
//!  │       finalize_non_registry_manifest  ★                                  │
//!  │       package_dir = <cache>/<name>/_http_<sha256(url)[:16]>/             │
//!  │       commit_cache_dir_atomic  ★  (stage → rename → _resolved marker)   │
//!  │       │                                                                  │
//!  │       ▼                                                                  │
//!  │  ResolvedPackage { dist.tarball = url, … }    ── bytes dropped here      │
//!  └───────│──────────────────────────────────────────────────────────────────┘
//!          ▼  (lockfile)
//!  ┌── Install phase (pm/util/downloader.rs) ─────────────────────────────────┐
//!  │  resolve_cache_path(name, version, url)                                  │
//!  │       ├─ is_git_url?           → git_cache_lookup                        │
//!  │       ├─ http_tarball_cache_lookup  → <name>/_http_<hash>/_resolved → ✓ │
//!  │       └─ (fall through)        → download_to_cache  (registry path)     │
//!  │                                                                          │
//!  │  cloner:  clonefile (mac) / hardlink (linux)                             │
//!  │       ~/.cache/nm/<name>/_http_<hash>/package/  →  node_modules/<name>/  │
//!  └──────────────────────────────────────────────────────────────────────────┘
//!
//!  Cache layout:
//!
//!    ~/.cache/nm/
//!    └── foo/
//!        ├── 1.2.3/                       registry tarball slot (untouched)
//!        └── _http_<sha256(url)[:16]>/    HTTP tarball slot
//!            ├── _resolved
//!            └── package/
//!                └── package.json
//!
//!  Legend:
//!    ★ shared with the git resolver via `super::common`
//!      (DedupCache, dedup_init, finalize_non_registry_manifest,
//!       validate_package_name, commit_cache_dir_atomic)
//!    ☆ shared with registry manifest fetching via `crate::service::fetch`
//!      (FetchError, classify_reqwest_error, classify_status, retry_strategy)
//! ```
//!
//! [`download_to_cache`]: https://github.com/utooland/utoo/blob/main/crates/pm/src/util/downloader.rs

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio_retry::RetryIf;

use super::common::{DedupCache, dedup_init};
use super::tar::commit_tarball_bytes;
use crate::model::manifest::CoreVersionManifest;
use crate::service::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use crate::service::http::get_client;
use crate::traits::registry::ResolvedPackage;

/// Session-scoped dedup cache: one fetch per URL even under concurrent BFS.
pub(crate) type HttpFetchCache = DedupCache<CoreVersionManifest>;

/// `<prefix><16 hex>` from `sha256(bytes)`. Used by the two non-registry
/// cache slots (`_http_<url>`, `_file_<path>`) to stay visually distinct
/// from `<name>/<version>/` and 40-char git commit shas.
fn cache_slot(prefix: &str, bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(prefix.len() + 16);
    out.push_str(prefix);
    for b in &digest[..8] {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub fn http_cache_slot(url: &str) -> String {
    cache_slot("_http_", url.as_bytes())
}

/// Cache slot for a local tarball keyed on its absolute path. Path is
/// normalized via `Path::components()` first so BFS (`base.join("./foo.tgz")`)
/// and install (`cwd.join("foo.tgz")` from a root-relative lockfile entry)
/// hash to the same slot.
pub fn file_cache_slot(abs_path: &Path) -> String {
    let normalized: PathBuf = abs_path.components().collect();
    cache_slot("_file_", normalized.as_os_str().as_encoded_bytes())
}

fn fetch_and_extract_blocking(
    cache_dir: &Path,
    url: &str,
    tarball_bytes: Bytes,
) -> Result<CoreVersionManifest> {
    commit_tarball_bytes(
        cache_dir,
        tarball_bytes.as_ref(),
        url.to_string(),
        &http_cache_slot(url),
    )
}

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

/// Resolve an HTTP tarball spec to a [`ResolvedPackage`] and seed the cache.
///
/// BFS extracts to `<cache_dir>/<name>/_http_<hash>/` so the install phase
/// finds a cache hit and never re-downloads.
pub(crate) async fn resolve_http_dep(
    cache_dir: Option<&Path>,
    url: &str,
    fetch_cache: &HttpFetchCache,
) -> Result<ResolvedPackage> {
    let cache_dir =
        cache_dir.ok_or_else(|| anyhow!("cache_dir required for http dependency resolution"))?;

    let url_owned = url.to_string();
    let cache_dir_owned = cache_dir.to_path_buf();
    let manifest = dedup_init(fetch_cache, url_owned.clone(), move || async move {
        let bytes = download_tarball(&url_owned).await?;
        tokio::task::spawn_blocking(move || {
            fetch_and_extract_blocking(&cache_dir_owned, &url_owned, bytes)
        })
        .await
        .context("http tarball extractor task failed")?
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
    fn slot_name_is_stable_and_url_specific() {
        let a = http_cache_slot("https://example.com/foo.tgz");
        let b = http_cache_slot("https://example.com/foo.tgz");
        let c = http_cache_slot("https://example.com/foo.tgz?v=2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("_http_"));
        assert_eq!(a.len(), "_http_".len() + 16);
    }

    #[test]
    fn extracts_to_url_hashed_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = br#"{"name":"demo","version":"1.2.3","scripts":{"install":"echo hi"}}"#;
        let bytes = make_targz(&[("package/package.json", pkg)]);
        let url = "https://example.com/demo.tgz";

        let manifest = fetch_and_extract_blocking(tmp.path(), url, bytes).unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.has_install_script, Some(true));
        assert_eq!(manifest.dist.tarball.as_deref(), Some(url));

        let expected_dir = tmp.path().join("demo").join(http_cache_slot(url));
        assert!(expected_dir.join("_resolved").exists());
        assert!(expected_dir.join("package").join("package.json").exists());
    }

    #[test]
    fn different_urls_get_separate_slots_same_name() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = br#"{"name":"demo","version":"1.0.0"}"#;
        let url_a = "https://a.example.com/demo.tgz";
        let url_b = "https://b.example.com/demo.tgz";

        fetch_and_extract_blocking(
            tmp.path(),
            url_a,
            make_targz(&[("package/package.json", pkg)]),
        )
        .unwrap();
        fetch_and_extract_blocking(
            tmp.path(),
            url_b,
            make_targz(&[("package/package.json", pkg)]),
        )
        .unwrap();

        let slot_a = tmp.path().join("demo").join(http_cache_slot(url_a));
        let slot_b = tmp.path().join("demo").join(http_cache_slot(url_b));
        assert_ne!(slot_a, slot_b);
        assert!(slot_a.join("_resolved").exists());
        assert!(slot_b.join("_resolved").exists());
    }

    #[test]
    fn warm_cache_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = make_targz(&[(
            "package/package.json",
            br#"{"name":"demo","version":"0.0.1"}"#,
        )]);
        let url = "https://example.com/x.tgz";

        fetch_and_extract_blocking(tmp.path(), url, bytes.clone()).unwrap();
        fetch_and_extract_blocking(tmp.path(), url, bytes).unwrap();
    }

    #[test]
    fn rejects_tarball_without_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = make_targz(&[("package/README.md", b"hi")]);
        let err = fetch_and_extract_blocking(tmp.path(), "u", bytes).unwrap_err();
        assert!(err.to_string().contains("package.json not found"));
    }

    #[test]
    fn shallowest_package_json_wins() {
        let tmp = tempfile::tempdir().unwrap();
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
        let manifest = fetch_and_extract_blocking(tmp.path(), "u", bytes).unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.0.0");
    }
}
