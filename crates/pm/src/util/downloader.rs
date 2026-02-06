use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest::StatusCode;
use std::io::Cursor;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio_retry::RetryIf;

use super::oncemap::OnceMap;
use super::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};

// Global downloader client - no pool limit, concurrency controlled by OnceMap
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

// OnceMap to ensure each (url, dest) pair is only downloaded once
static DOWNLOAD_ONCE: Lazy<OnceMap<(String, PathBuf), ()>> = Lazy::new(OnceMap::new);

const MIN_ESTIMATED_SIZE: usize = 16;
const MAX_ESTIMATED_SIZE: usize = 512 * 1024 * 1024; // 512MB
const DECOMPRESSION_RETRY_FACTOR: usize = 4;

/// Download and extract a tarball to the destination directory.
///
/// Uses OnceMap to ensure each (url, dest) pair is only downloaded once,
/// even when called concurrently from multiple tasks.
pub async fn download(url: &str, dest: &Path) -> Result<()> {
    let key = (url.to_string(), dest.to_path_buf());

    DOWNLOAD_ONCE
        .get_or_init(key, || async {
            let bytes = download_bytes(url).await.ok()?;
            extract_and_write(bytes, dest).await.ok()?;
            Some(())
        })
        .await
        .map(|_| ())
        .ok_or_else(|| anyhow::anyhow!("Download failed: {}", url))
}

/// Download tarball bytes (network phase)
pub async fn download_bytes(url: &str) -> Result<Bytes> {
    let retry_count = AtomicU32::new(0);
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let attempt = retry_count.fetch_add(1, Ordering::Relaxed);

            let response = match DOWNLOADER_CLIENT.get(url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Retry {}/10 - Network error: {}, url: {}",
                        attempt + 1,
                        e,
                        url
                    );
                    return Err(RetryableError::Temporary(format!("Network error: {e}")));
                }
            };

            match response.status() {
                StatusCode::OK => {
                    let bytes = response.bytes().await.map_err(|e| {
                        tracing::warn!(
                            "Retry {}/10 - Stream error: {}, url: {}",
                            attempt + 1,
                            e,
                            url
                        );
                        RetryableError::Temporary(format!("Stream error: {e}"))
                    })?;
                    if attempt > 0 {
                        tracing::info!("Retry succeeded on attempt {}, url: {}", attempt + 1, url);
                    }
                    Ok(bytes)
                }
                StatusCode::NOT_FOUND => {
                    tracing::debug!("URL not found {url}");
                    Err(RetryableError::Permanent(format!("URL not found {url}")))
                }
                status => {
                    tracing::warn!("Retry {}/10 - HTTP {}, url: {}", attempt + 1, status, url);
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {status}, url: {url}"
                    )))
                }
            }
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await
    .context("Download failed after retries")
}

/// Extract gzip tarball and write to destination
pub async fn extract_and_write(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    // Check if already resolved (warm cache scenario)
    let resolved_path = dest.join("_resolved");
    if crate::fs::try_exists(&resolved_path).await? {
        tracing::debug!("Extract skipped, already resolved: {}", dest.display());
        return Ok(());
    }

    crate::fs::create_dir_all(dest)
        .await
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    extract_tarball(gzip_bytes, dest).await
}

/// Estimate uncompressed size from gzip footer (last 4 bytes store original size mod 2^32)
fn estimate_uncompressed_size(gzip_data: &[u8]) -> usize {
    if gzip_data.len() < 4 {
        return gzip_data.len() * 10; // fallback estimate
    }
    let last_4 = &gzip_data[gzip_data.len() - 4..];
    let size = u32::from_le_bytes([last_4[0], last_4[1], last_4[2], last_4[3]]) as usize;
    // Sanity check: if size is 0 or too small, use a reasonable estimate
    if !(MIN_ESTIMATED_SIZE..=MAX_ESTIMATED_SIZE).contains(&size) {
        gzip_data.len() * 10
    } else {
        size
    }
}

struct ExtractedEntry {
    path: PathBuf,
    content: Vec<u8>,
    mode: u32,
}

/// Extract tarball using libdeflate + rayon (no tokio blocking pool)
async fn extract_tarball(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    let estimated_size = estimate_uncompressed_size(&gzip_bytes);
    let dest_owned = dest.to_path_buf();

    let (tx, rx) = tokio::sync::oneshot::channel();

    rayon::spawn(move || {
        let result = extract_tarball_sync(gzip_bytes, estimated_size, &dest_owned);
        let _ = tx.send(result);
    });

    rx.await.with_context(|| "Extract task panicked")?
}

/// Synchronous extraction: decompress + parse + parallel write, all on rayon
fn extract_tarball_sync(gzip_bytes: Bytes, estimated_size: usize, dest: &Path) -> Result<()> {
    use rayon::prelude::*;
    use std::collections::HashSet;
    use std::fs;
    use std::io::{Read, Write};

    // Decompress gzip using libdeflate
    let mut output = vec![0u8; estimated_size];

    let mut decompressor = libdeflater::Decompressor::new();

    let actual_size = match decompressor.gzip_decompress(&gzip_bytes, &mut output) {
        Ok(size) => size,
        Err(libdeflater::DecompressionError::InsufficientSpace) => {
            let new_size = estimated_size * DECOMPRESSION_RETRY_FACTOR;
            output.resize(new_size, 0);
            decompressor
                .gzip_decompress(&gzip_bytes, &mut output)
                .with_context(|| "gzip decompression failed")?
        }
        Err(e) => return Err(anyhow::anyhow!("gzip decompression failed: {}", e)),
    };
    output.truncate(actual_size);

    // Parse tar entries
    let cursor = Cursor::new(&output[..]);
    let mut archive = tar::Archive::new(cursor);
    let mut entries = Vec::new();

    for entry_result in archive.entries()? {
        let mut entry = entry_result.with_context(|| "Failed to read tar entry")?;
        let path = entry
            .path()
            .with_context(|| "Failed to get entry path")?
            .into_owned();
        let full_path = dest.join(&path);

        if !entry.header().entry_type().is_dir() {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .with_context(|| format!("Failed to read tar entry: {}", path.display()))?;

            let mode = entry.header().mode().unwrap_or(0o644);
            entries.push(ExtractedEntry {
                path: full_path,
                content,
                mode,
            });
        }
    }

    // Create directories
    let mut created_dirs = HashSet::new();
    for entry in entries.iter() {
        if let Some(parent) = entry.path.parent()
            && created_dirs.insert(parent.to_path_buf())
        {
            fs::create_dir_all(parent).ok();
        }
    }

    // Write files in parallel using rayon
    entries.par_iter().try_for_each(|entry| -> Result<()> {
        let mut file = fs::File::create(&entry.path)
            .with_context(|| format!("Failed to create: {}", entry.path.display()))?;
        file.write_all(&entry.content)
            .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

        #[cfg(unix)]
        {
            let perms = fs::Permissions::from_mode(entry.mode);
            fs::set_permissions(&entry.path, perms).ok();
        }
        Ok(())
    })?;

    // Set directory permissions
    #[cfg(unix)]
    {
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(dest, perms).ok();
    }

    // Create resolution marker
    fs::File::create(dest.join("_resolved"))
        .with_context(|| format!("Failed to create resolution marker in: {}", dest.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use mockito::Server;
    use std::io::Write;
    use tar::Builder;
    use tempfile::TempDir;
    use tokio::task;

    // Helper to create a simple tar.gz archive in memory
    fn create_tar_gz() -> Vec<u8> {
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);
            let mut header = tar::Header::new_gnu();
            let content = b"hello world";
            header.set_path("file.txt").unwrap();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append(&header, &content[..]).unwrap();
            tar.finish().unwrap();
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_data).unwrap();
        encoder.finish().unwrap()
    }

    #[tokio::test]
    async fn test_download_idempotent() {
        let tar_gz = create_tar_gz();
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/pkg.tgz")
            .with_status(200)
            .with_header("content-type", "application/gzip")
            .with_body(tar_gz.clone())
            .expect(1)
            .create_async()
            .await;

        let url = format!("{}/pkg.tgz", server.url());
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");
        let n = 8;
        let mut handles = Vec::new();
        for _ in 0..n {
            let url = url.clone();
            let dest = dest.clone();
            handles.push(task::spawn(async move {
                download(&url, &dest).await.unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        // _resolved file should exist
        assert!(dest.join("_resolved").exists());
        // Extracted file should exist
        assert!(dest.join("file.txt").exists());
        // All concurrent calls should succeed and not corrupt the output
        let content = crate::fs::read_to_string(dest.join("file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");
        _m.assert();
    }
}
