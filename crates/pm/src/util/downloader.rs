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
use tokio::fs::File;
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

/// Sanitize a path for Windows compatibility.
/// Windows doesn't allow: < > : " | ? * and control characters (0-31)
/// We replace them with underscore to avoid extraction failures.
#[cfg(windows)]
fn sanitize_path_for_windows(base: &Path, relative: &Path) -> PathBuf {
    use std::path::Component;

    let mut result = base.to_path_buf();
    for comp in relative.components() {
        match comp {
            Component::Normal(os_str) => {
                let s = os_str.to_string_lossy();
                let sanitized: String = s
                    .chars()
                    .map(|c| {
                        if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 32 {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect();
                result.push(&sanitized);
            }
            Component::ParentDir => result.push(".."),
            Component::CurDir => result.push("."),
            _ => {}
        }
    }
    result
}

#[cfg(not(windows))]
fn sanitize_path_for_windows(base: &Path, relative: &Path) -> PathBuf {
    base.join(relative)
}

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
async fn download_bytes(url: &str) -> Result<Bytes> {
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
async fn extract_and_write(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
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

/// Extract tarball using libdeflate for better performance
async fn extract_tarball(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    let estimated_size = estimate_uncompressed_size(&gzip_bytes);
    let dest_owned = dest.to_path_buf();

    let entries: Vec<ExtractedEntry> = tokio::task::spawn_blocking(move || -> Result<Vec<_>> {
        use std::io::Read;

        // Allocate buffer for decompression
        // SAFETY: libdeflater will write to the buffer, we don't need to initialize
        let mut output = Vec::with_capacity(estimated_size);
        #[allow(clippy::uninit_vec)]
        unsafe {
            output.set_len(estimated_size)
        };

        let mut decompressor = libdeflater::Decompressor::new();

        let actual_size = match decompressor.gzip_decompress(&gzip_bytes, &mut output) {
            Ok(size) => size,
            Err(libdeflater::DecompressionError::InsufficientSpace) => {
                // Buffer too small, retry with larger buffer
                let new_size = estimated_size * DECOMPRESSION_RETRY_FACTOR;
                output.reserve(new_size - output.len());
                #[allow(clippy::uninit_vec)]
                unsafe {
                    output.set_len(new_size)
                };
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
            let full_path = sanitize_path_for_windows(&dest_owned, &path);

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

        Ok(entries)
    })
    .await
    .with_context(|| "Decompression task panicked")??;

    // Write files sequentially within spawn_blocking
    tokio::task::spawn_blocking(move || -> Result<()> {
        use std::collections::HashSet;
        use std::fs;
        use std::io::Write;

        // First, create all directories
        let mut created_dirs = HashSet::new();
        for entry in entries.iter() {
            if let Some(parent) = entry.path.parent()
                && created_dirs.insert(parent.to_path_buf())
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }
        }

        // Then write files sequentially
        for entry in &entries {
            let mut file = fs::File::create(&entry.path)
                .with_context(|| format!("Failed to create: {}", entry.path.display()))?;
            file.write_all(&entry.content)
                .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(entry.mode);
                fs::set_permissions(&entry.path, perms).with_context(|| {
                    format!("Failed to set permissions: {}", entry.path.display())
                })?;
            }
        }
        Ok(())
    })
    .await
    .with_context(|| "Write task panicked")??;

    // Set directory permissions and create resolution marker
    set_dir_permissions(dest).await?;
    File::create(&dest.join("_resolved"))
        .await
        .with_context(|| format!("Failed to create resolution marker in: {}", dest.display()))?;

    Ok(())
}

/// Set directory permissions (cross-platform)
#[cfg(unix)]
async fn set_dir_permissions(path: &Path) -> Result<()> {
    let permissions = std::fs::Permissions::from_mode(0o755);
    crate::fs::set_permissions(path, permissions)
        .await
        .with_context(|| format!("Failed to set directory permissions: {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_dir_permissions(_path: &Path) -> Result<()> {
    // Windows doesn't need Unix-style permissions
    Ok(())
}

#[derive(Debug)]
struct ExtractedEntry {
    path: PathBuf,
    content: Vec<u8>,
    mode: u32, // File permission mode
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
