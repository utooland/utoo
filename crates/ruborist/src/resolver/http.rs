//! HTTP(S) tarball resolver for `https://…/pkg.tgz` dependency specs.
//!
//! # Why this is much smaller than the git resolver
//!
//! Git specs (`git+…#sha`) can't be handled by the install-phase downloader
//! because it can't clone. So git has to clone + extract up-front during BFS
//! and seed `<cache_dir>/<name>/<sha>/_resolved` so install finds a cache hit.
//!
//! HTTP tarball URLs are *ordinary tarball URLs*. Once BFS returns a
//! [`ResolvedPackage`] with `dist.tarball = url`, pm's install-phase
//! [`download_to_cache`] handles fetching and extraction through the same
//! code path it already uses for every registry tarball — no pre-extraction,
//! no `_resolved` seeding, no atomic-commit ritual.
//!
//! What BFS needs from us is just the manifest (`name`, `version`, deps),
//! so we download the bytes, find + parse `package.json` inside the tarball,
//! and throw the rest away. Install will re-download (one extra cold-path
//! fetch per unique URL) and extract via pm's tuned libdeflate+rayon pipeline.
//!
//! # Flow
//!
//! ```text
//!   package.json:  "foo": "https://example.com/foo-1.2.3.tgz"
//!                                   │
//!  ┌── BFS resolution (this module) ─────────────────────────────────────────┐
//!  │                                ▼                                        │
//!  │  resolve_http_dep(url, &fetch_cache)                                    │
//!  │      │                                                                  │
//!  │      ▼                                                                  │
//!  │  HttpFetchCache  ── dedup_init  ★                                       │
//!  │      │            (one fetch per URL across BFS)                        │
//!  │      ▼                                                                  │
//!  │  download_tarball  ── FetchError + classify_*  ☆                        │
//!  │      │                                                                  │
//!  │      ▼  Bytes                                                           │
//!  │  spawn_blocking → parse_manifest:                                       │
//!  │      gzip_decompress (libdeflater)                                      │
//!  │      find_package_json (single-pass tar scan)                           │
//!  │      finalize_non_registry_manifest  ★                                  │
//!  │      │                                                                  │
//!  │      ▼                                                                  │
//!  │  ResolvedPackage { dist.tarball = url, … }    ── bytes dropped here     │
//!  └──────│──────────────────────────────────────────────────────────────────┘
//!         ▼  (lockfile)
//!  ┌── Install phase (pm/util/downloader.rs) ────────────────────────────────┐
//!  │  download_to_cache(name, version, url)                                  │
//!  │      │                                                                  │
//!  │      ├─ ~/.cache/nm/<name>/<version>/_resolved exists?  → return        │
//!  │      │                                                                  │
//!  │      └─ miss: download_bytes → extract_and_write (libdeflate + rayon)   │
//!  │              writes ~/.cache/nm/<name>/<version>/{package/, _resolved}  │
//!  │                                                                         │
//!  │  cloner:  clonefile (mac) / hardlink (linux)                            │
//!  │      ~/.cache/nm/<name>/<version>/package/  →  node_modules/<name>/     │
//!  └─────────────────────────────────────────────────────────────────────────┘
//!
//!  Cache layout (identical to registry tarballs and git deps):
//!
//!    ~/.cache/nm/
//!    └── foo/
//!        └── 1.2.3/              cache slot = <name>/<version>
//!            ├── _resolved        install-phase marker (BFS does NOT write this)
//!            └── package/         npm-canonical wrapper
//!                ├── package.json
//!                └── ...
//!
//!  Legend:
//!    ★ shared with the git resolver via `super::common`
//!      (DedupCache, dedup_init, finalize_non_registry_manifest, validate_package_name)
//!    ☆ shared with registry manifest fetching via `crate::service::fetch`
//!      (FetchError, classify_reqwest_error, classify_status, retry_strategy)
//! ```
//!
//! [`download_to_cache`]: https://github.com/utooland/utoo/blob/main/crates/pm/src/util/downloader.rs

use std::io::Read;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use tokio_retry::RetryIf;

use super::common::{DedupCache, dedup_init, finalize_non_registry_manifest};
use crate::model::manifest::CoreVersionManifest;
use crate::service::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use crate::service::http::get_client;
use crate::traits::registry::ResolvedPackage;

/// Session-scoped dedup cache: one fetch per URL even under concurrent BFS.
pub(crate) type HttpFetchCache = DedupCache<CoreVersionManifest>;

/// Estimate uncompressed size from the gzip ISIZE footer, bounded by sanity
/// limits to deflect crafted-footer allocation bombs.
fn estimate_uncompressed_size(gzip_bytes: &[u8]) -> usize {
    const MIN: usize = 16;
    const MAX: usize = 512 * 1024 * 1024; // 512 MiB ceiling
    if gzip_bytes.len() < 4 {
        return gzip_bytes.len() * 10;
    }
    let footer = &gzip_bytes[gzip_bytes.len() - 4..];
    let size = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) as usize;
    if (MIN..=MAX).contains(&size) {
        size
    } else {
        gzip_bytes.len() * 10
    }
}

fn gzip_decompress(gzip_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decompressor = libdeflater::Decompressor::new();
    let estimated = estimate_uncompressed_size(gzip_bytes);
    let mut output = vec![0u8; estimated];
    let actual = match decompressor.gzip_decompress(gzip_bytes, &mut output) {
        Ok(n) => n,
        Err(libdeflater::DecompressionError::InsufficientSpace) => {
            output.resize(estimated.saturating_mul(4).max(1024), 0);
            decompressor
                .gzip_decompress(gzip_bytes, &mut output)
                .context("gzip decompression failed (even with 4x buffer)")?
        }
        Err(e) => return Err(anyhow!("gzip decompression failed: {e:?}")),
    };
    output.truncate(actual);
    Ok(output)
}

/// Find the canonical (shallowest) `package.json` within a decompressed tar
/// stream and return its raw bytes.
///
/// npm tarballs conventionally use a `package/` wrapper so `package.json` lives
/// at depth 2; deeper matches are accepted as fallback for non-standard
/// layouts (`package/sub/package.json` loses to `package/package.json`).
fn find_package_json(tar_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    let mut best: Option<(usize, Vec<u8>)> = None;

    for entry_result in archive.entries().context("failed to iterate tar entries")? {
        let mut entry = entry_result.context("failed to read tar entry")?;
        let path = entry
            .path()
            .context("failed to read tar entry path")?
            .into_owned();

        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "package.json") {
            let depth = path.components().count();
            if best.as_ref().is_none_or(|(d, _)| depth < *d) {
                let mut content = Vec::new();
                entry
                    .read_to_end(&mut content)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                best = Some((depth, content));
            }
        }
    }

    best.map(|(_, b)| b)
        .ok_or_else(|| anyhow!("package.json not found in tarball"))
}

fn parse_manifest(tarball_bytes: &[u8], url: &str) -> Result<CoreVersionManifest> {
    let decompressed = gzip_decompress(tarball_bytes)?;
    let pkg_bytes = find_package_json(&decompressed)?;
    let mut manifest: CoreVersionManifest =
        serde_json::from_slice(&pkg_bytes).context("failed to parse package.json from tarball")?;
    finalize_non_registry_manifest(&mut manifest, url.to_string())?;
    Ok(manifest)
}

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

/// Resolve an HTTP tarball spec to a [`ResolvedPackage`].
pub(crate) async fn resolve_http_dep(
    url: &str,
    fetch_cache: &HttpFetchCache,
) -> Result<ResolvedPackage> {
    let url_owned = url.to_string();
    let manifest = dedup_init(fetch_cache, url_owned.clone(), move || async move {
        let bytes = download_tarball(&url_owned).await?;
        tokio::task::spawn_blocking(move || parse_manifest(&bytes, &url_owned).map(Arc::new))
            .await
            .context("http tarball parse task failed")?
    })
    .await?;

    Ok(ResolvedPackage {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        manifest,
    })
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
    fn parses_manifest_and_sets_dist() {
        let pkg = br#"{"name":"demo","version":"1.2.3","scripts":{"install":"echo hi"}}"#;
        let bytes = make_targz(&[("package/package.json", pkg)]);
        let manifest = parse_manifest(&bytes, "https://example.com/demo.tgz").unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.has_install_script, Some(true));
        assert_eq!(
            manifest.dist.tarball.as_deref(),
            Some("https://example.com/demo.tgz")
        );
    }

    #[test]
    fn rejects_tarball_without_package_json() {
        let bytes = make_targz(&[("package/README.md", b"hi")]);
        let err = parse_manifest(&bytes, "u").unwrap_err();
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
        let manifest = parse_manifest(&bytes, "u").unwrap();
        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.version, "1.0.0");
    }
}
