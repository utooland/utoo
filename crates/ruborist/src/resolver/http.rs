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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio_retry::RetryIf;

use super::common::{
    DedupCache, commit_cache_dir_atomic, dedup_init, finalize_non_registry_manifest,
};
use crate::model::manifest::CoreVersionManifest;
use crate::service::fetch::{
    FetchError, classify_reqwest_error, classify_status, is_retryable, retry_strategy,
};
use crate::service::http::get_client;
use crate::traits::registry::ResolvedPackage;

/// Session-scoped dedup cache: one fetch per URL even under concurrent BFS.
pub(crate) type HttpFetchCache = DedupCache<CoreVersionManifest>;

/// Derive the cache sub-directory name for an HTTP(S) tarball URL.
///
/// Returns `"_http_<first-16-hex-of-sha256(url)>"`. The `_http_` prefix and
/// 16-char suffix keep this visually distinct from a 40-char git commit sha
/// and from a semver string, so `<cache>/<name>/<slot>/` entries never
/// collide between resolver families.
///
/// Both ruborist (writing) and pm (lookup at install time) call this helper
/// to agree on the same slot for a given URL.
pub fn http_cache_slot(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut out = String::with_capacity(22);
    out.push_str("_http_");
    for b in &digest[..8] {
        let _ = write!(out, "{b:02x}");
    }
    out
}

// ============================================================================
// Tarball scan + write
// ============================================================================

/// Decoded tar entry held in memory until the package name/version is known
/// and the final cache path can be resolved.
#[derive(Debug)]
struct TarEntry {
    rel_path: PathBuf,
    content: Vec<u8>,
    mode: u32,
    is_dir: bool,
}

/// Single-pass tar scan: collects safe entries **and** the shallowest
/// `package.json` blob for manifest extraction.
///
/// npm tarballs put everything under a top-level `package/` dir, so the
/// `package.json` at depth 2 is canonical. Deeper matches win only as
/// fallback for non-standard layouts.
fn scan_tarball(tar_bytes: &[u8]) -> Result<(Vec<TarEntry>, Vec<u8>)> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    archive.set_preserve_permissions(false);

    let mut entries = Vec::new();
    let mut manifest_blob: Option<(usize, Vec<u8>)> = None;

    for entry_result in archive.entries().context("failed to iterate tar entries")? {
        let mut entry = entry_result.context("failed to read tar entry")?;
        let rel_path = entry
            .path()
            .context("failed to read tar entry path")?
            .into_owned();

        if rel_path.is_absolute()
            || rel_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            tracing::warn!(
                "Skipping tar entry with unsafe path: {}",
                rel_path.display()
            );
            continue;
        }

        let is_dir = entry.header().entry_type().is_dir();
        let mode = entry.header().mode().unwrap_or(0o644);
        // Don't pre-reserve from entry.size() — a crafted header could trigger
        // huge allocations. Let Vec grow naturally.
        let mut content = Vec::new();
        if !is_dir {
            entry
                .read_to_end(&mut content)
                .with_context(|| format!("failed to read tar entry {}", rel_path.display()))?;
        }

        if !is_dir && rel_path.file_name().is_some_and(|n| n == "package.json") {
            let depth = rel_path.components().count();
            if manifest_blob.as_ref().is_none_or(|(d, _)| depth < *d) {
                manifest_blob = Some((depth, content.clone()));
            }
        }

        entries.push(TarEntry {
            rel_path,
            content,
            mode,
            is_dir,
        });
    }

    let manifest_blob = manifest_blob
        .map(|(_, b)| b)
        .ok_or_else(|| anyhow!("package.json not found in tarball"))?;

    Ok((entries, manifest_blob))
}

/// Write collected tar entries into `dest`, preserving the archive's
/// directory layout (including the typical `package/` wrapper) so the
/// install phase can locate the real package root via `find_real_src`.
///
/// Preserves the executable bit on Unix — npm packages rely on it for
/// binaries under `bin/`.
fn write_entries(entries: &[TarEntry], dest: &Path) -> Result<()> {
    for entry in entries {
        let full_path = dest.join(&entry.rel_path);
        if entry.is_dir {
            std::fs::create_dir_all(&full_path)
                .with_context(|| format!("failed to create directory {}", full_path.display()))?;
            continue;
        }
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        std::fs::write(&full_path, &entry.content)
            .with_context(|| format!("failed to write {}", full_path.display()))?;

        #[cfg(unix)]
        if entry.mode != 0o644 {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&full_path, std::fs::Permissions::from_mode(entry.mode));
        }
    }
    Ok(())
}

// ============================================================================
// gzip decompression (libdeflater — matches pm's extractor)
// ============================================================================

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

// ============================================================================
// Blocking core: decompress → parse → extract to URL-hashed cache slot
// ============================================================================

fn fetch_and_extract_blocking(
    cache_dir: &Path,
    url: &str,
    tarball_bytes: Bytes,
) -> Result<CoreVersionManifest> {
    let decompressed = gzip_decompress(tarball_bytes.as_ref())?;
    let (entries, manifest_blob) = scan_tarball(&decompressed)?;

    let mut manifest: CoreVersionManifest = serde_json::from_slice(&manifest_blob)
        .context("failed to parse package.json from tarball")?;
    finalize_non_registry_manifest(&mut manifest, url.to_string())?;

    let package_dir = cache_dir.join(&manifest.name).join(http_cache_slot(url));
    if package_dir.join("_resolved").exists() {
        return Ok(manifest);
    }

    commit_cache_dir_atomic(&package_dir, |stage| write_entries(&entries, stage))?;

    Ok(manifest)
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
            fetch_and_extract_blocking(&cache_dir_owned, &url_owned, bytes).map(Arc::new)
        })
        .await
        .context("http tarball extractor task failed")?
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
