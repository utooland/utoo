//! HTTP(S) tarball resolver for `https://…/pkg.tgz` dependency specs.
//!
//! Downloads the tarball once, reads `package.json` for name/version, extracts
//! to `<cache_dir>/<name>/<version>/`, and writes a `_resolved` marker so the
//! install phase can cache-hit via [`download_to_cache`] without re-fetching.
//!
//! [`download_to_cache`]: https://github.com/utooland/utoo/blob/main/crates/pm/src/util/downloader.rs

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use flate2::read::GzDecoder;
use tokio_retry::RetryIf;
use tokio_retry::strategy::FixedInterval;

use crate::model::manifest::{CoreVersionManifest, Dist};
use crate::service::http::get_client;
use crate::traits::registry::ResolvedPackage;

/// Session-scoped dedup cache: one fetch per URL even under concurrent BFS.
pub type HttpFetchCache =
    tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::OnceCell<Arc<HttpFetchResult>>>>>;

#[derive(Debug, Clone)]
pub struct HttpFetchResult {
    pub path: PathBuf,
    pub manifest: CoreVersionManifest,
}

// ============================================================================
// Helpers
// ============================================================================

/// Reject package names with path-traversal components before using them in
/// cache paths (`cache_dir.join(name)`).
fn validate_package_name(name: &str) -> Result<()> {
    let name_path = PathBuf::from(name);
    if name_path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Err(anyhow!(
            "Suspicious package name '{}' — refusing to use for cache path",
            name
        ));
    }
    Ok(())
}

/// Decoded tar entry held in memory until the package name/version is known
/// and the final cache path can be resolved.
struct TarEntry {
    rel_path: PathBuf,
    content: Vec<u8>,
    is_dir: bool,
}

/// Single-pass tar scan: collects safe entries and the best `package.json` blob
/// to use for manifest extraction.
///
/// npm tarballs put everything under a top-level dir (usually `package/`), so
/// the `package.json` at depth 2 is canonical. Deeper matches are accepted as
/// fallback for non-standard layouts.
fn scan_tarball(tar_bytes: &[u8]) -> Result<(Vec<TarEntry>, Vec<u8>)> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    archive.set_preserve_permissions(false);

    let mut entries = Vec::new();
    let mut manifest_blob: Option<(usize, Vec<u8>)> = None;

    for entry_result in archive.entries()? {
        let mut entry = entry_result.context("failed to read tar entry")?;
        let rel_path = entry
            .path()
            .context("failed to get tar entry path")?
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
        let mut content = Vec::new();
        if !is_dir {
            content.reserve(entry.size() as usize);
            entry
                .read_to_end(&mut content)
                .with_context(|| format!("failed to read tar entry {}", rel_path.display()))?;
        }

        if !is_dir && rel_path.file_name().is_some_and(|n| n == "package.json") {
            let depth = rel_path.components().count();
            // Shallowest match wins — the npm-conventional `package/package.json`
            // at depth 2 beats any nested package.json that might exist inside
            // test fixtures or bundled dependencies.
            if manifest_blob.as_ref().is_none_or(|(d, _)| depth < *d) {
                manifest_blob = Some((depth, content.clone()));
            }
        }

        entries.push(TarEntry {
            rel_path,
            content,
            is_dir,
        });
    }

    let manifest_blob = manifest_blob
        .map(|(_, b)| b)
        .ok_or_else(|| anyhow!("package.json not found in tarball"))?;

    Ok((entries, manifest_blob))
}

/// Write collected tar entries into `dest`, preserving the archive's directory
/// layout (including the typical `package/` wrapper) so the install phase can
/// locate the real package root via `find_real_src`.
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
    }
    Ok(())
}

// ============================================================================
// Blocking core
// ============================================================================

fn fetch_and_extract_blocking(
    cache_dir: &Path,
    url: &str,
    tarball_bytes: Bytes,
) -> Result<HttpFetchResult> {
    let mut gz = GzDecoder::new(tarball_bytes.as_ref());
    let mut decompressed = Vec::with_capacity(tarball_bytes.len() * 4);
    gz.read_to_end(&mut decompressed)
        .context("failed to decompress tarball (gzip)")?;

    let (entries, manifest_blob) = scan_tarball(&decompressed)?;

    let mut manifest: CoreVersionManifest = serde_json::from_slice(&manifest_blob)
        .context("failed to parse package.json from tarball")?;

    if manifest.name.is_empty() {
        return Err(anyhow!("package.json in tarball is missing 'name' field"));
    }
    if manifest.version.is_empty() {
        tracing::debug!(
            "package.json in tarball '{}' missing 'version'; defaulting to 0.0.0",
            manifest.name
        );
        manifest.version = "0.0.0".to_string();
    }
    validate_package_name(&manifest.name)?;

    manifest.dist = Dist {
        tarball: Some(url.to_string()),
        integrity: None,
        ..Default::default()
    };
    manifest.has_install_script = Some(manifest.scripts.as_ref().is_some_and(|s| {
        s.contains_key("preinstall") || s.contains_key("install") || s.contains_key("postinstall")
    }));

    let package_dir = cache_dir.join(&manifest.name).join(&manifest.version);
    if package_dir.join("_resolved").exists() {
        return Ok(HttpFetchResult {
            path: package_dir,
            manifest,
        });
    }

    let parent_dir = package_dir
        .parent()
        .ok_or_else(|| anyhow!("package_dir has no parent"))?;
    std::fs::create_dir_all(parent_dir)
        .with_context(|| format!("failed to create cache dir {}", parent_dir.display()))?;

    let tmp_dir = tempfile::tempdir_in(parent_dir)
        .context("failed to create staging directory for http tarball cache")?;
    write_entries(&entries, tmp_dir.path())?;
    std::fs::write(tmp_dir.path().join("_resolved"), "")?;

    let tmp_path = tmp_dir.keep();
    match std::fs::rename(&tmp_path, &package_dir) {
        Ok(()) => {}
        Err(e)
            if e.kind() == std::io::ErrorKind::AlreadyExists
                || e.raw_os_error() == Some(libc::ENOTEMPTY) =>
        {
            // Another process committed its own staging dir first — discard ours.
            let _ = std::fs::remove_dir_all(&tmp_path);
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_path);
            return Err(anyhow!("failed to commit http tarball cache dir: {e}"));
        }
    }

    Ok(HttpFetchResult {
        path: package_dir,
        manifest,
    })
}

// ============================================================================
// Async public API
// ============================================================================

/// Retryable outcome: `Ok` on success; `Err(true)` for transient failures
/// (network, 5xx, 429); `Err(false)` for permanent ones (4xx, parse).
enum FetchAttempt {
    Ok(Bytes),
    Retry(anyhow::Error),
    Fatal(anyhow::Error),
}

async fn download_once(url: &str) -> FetchAttempt {
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return FetchAttempt::Fatal(e),
    };
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return FetchAttempt::Retry(anyhow!("{e}")),
    };
    let status = resp.status();
    if status.is_success() {
        return match resp.bytes().await {
            Ok(b) => FetchAttempt::Ok(b),
            Err(e) => FetchAttempt::Retry(anyhow!("stream error: {e}")),
        };
    }
    let err = anyhow!("HTTP {status} fetching tarball from {url}");
    if status.is_server_error() || status.as_u16() == 429 {
        FetchAttempt::Retry(err)
    } else {
        FetchAttempt::Fatal(err)
    }
}

/// Download the tarball bytes for `url` with retry on transient failures.
async fn download_tarball(url: &str) -> Result<Bytes> {
    let strategy = FixedInterval::new(Duration::from_millis(300)).take(3);
    RetryIf::spawn(
        strategy,
        || async {
            match download_once(url).await {
                FetchAttempt::Ok(b) => Ok(b),
                FetchAttempt::Retry(e) => {
                    tracing::warn!("tarball fetch retry: {url}: {e}");
                    Err((true, e))
                }
                FetchAttempt::Fatal(e) => Err((false, e)),
            }
        },
        |err: &(bool, anyhow::Error)| err.0,
    )
    .await
    .map_err(|(_, e)| e)
    .with_context(|| format!("failed to fetch tarball from {url}"))
}

// ============================================================================
// High-level resolver — called by BFS `process_dependency`
// ============================================================================

/// Resolve an HTTP tarball spec to a [`ResolvedPackage`].
pub(crate) async fn resolve_http_dep(
    cache_dir: Option<&Path>,
    url: &str,
    fetch_cache: &HttpFetchCache,
) -> Result<ResolvedPackage> {
    let cache_dir =
        cache_dir.ok_or_else(|| anyhow!("cache_dir required for http dependency resolution"))?;

    let cell = {
        let mut cache = fetch_cache.lock().await;
        cache
            .entry(url.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    let url_owned = url.to_string();
    let cache_dir_owned = cache_dir.to_path_buf();
    let result = cell
        .get_or_try_init(|| async move {
            let bytes = download_tarball(&url_owned).await?;
            tokio::task::spawn_blocking(move || {
                fetch_and_extract_blocking(&cache_dir_owned, &url_owned, bytes).map(Arc::new)
            })
            .await
            .context("http tarball extractor task failed")?
        })
        .await
        .cloned()?;

    Ok(ResolvedPackage {
        name: result.manifest.name.clone(),
        version: result.manifest.version.clone(),
        manifest: Arc::new(result.manifest.clone()),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    /// Build a gzip-compressed tarball containing `package/package.json`.
    fn make_tarball(pkg_json: &str) -> Bytes {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            let content = pkg_json.as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_path("package/package.json").unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append(&header, content).unwrap();
            tar.finish().unwrap();
        }
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&tar_data).unwrap();
        Bytes::from(enc.finish().unwrap())
    }

    #[test]
    fn extracts_and_reads_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_json = r#"{"name":"demo","version":"1.2.3","scripts":{"install":"echo hi"}}"#;
        let bytes = make_tarball(pkg_json);

        let result =
            fetch_and_extract_blocking(tmp.path(), "https://example.com/demo.tgz", bytes).unwrap();

        assert_eq!(result.manifest.name, "demo");
        assert_eq!(result.manifest.version, "1.2.3");
        assert_eq!(result.path, tmp.path().join("demo").join("1.2.3"));
        assert!(result.path.join("_resolved").exists());
        assert!(result.path.join("package").join("package.json").exists());
        assert_eq!(result.manifest.has_install_script, Some(true));
        assert_eq!(
            result.manifest.dist.tarball.as_deref(),
            Some("https://example.com/demo.tgz")
        );
    }

    #[test]
    fn warm_cache_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = make_tarball(r#"{"name":"demo","version":"0.0.1"}"#);

        let first =
            fetch_and_extract_blocking(tmp.path(), "https://example.com/x.tgz", bytes.clone())
                .unwrap();
        let second =
            fetch_and_extract_blocking(tmp.path(), "https://example.com/x.tgz", bytes).unwrap();
        assert_eq!(first.path, second.path);
    }

    #[test]
    fn rejects_tarball_without_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            let content = b"hi";
            let mut header = tar::Header::new_gnu();
            header.set_path("package/README.md").unwrap();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append(&header, &content[..]).unwrap();
            tar.finish().unwrap();
        }
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&tar_data).unwrap();
        let bytes = Bytes::from(enc.finish().unwrap());

        let err =
            fetch_and_extract_blocking(tmp.path(), "https://example.com/x.tgz", bytes).unwrap_err();
        assert!(err.to_string().contains("package.json not found"));
    }

    #[test]
    fn rejects_unsafe_package_name() {
        assert!(validate_package_name("../evil").is_err());
        assert!(validate_package_name("/etc/passwd").is_err());
        assert!(validate_package_name("@scope/pkg").is_ok());
        assert!(validate_package_name("lodash").is_ok());
    }
}
