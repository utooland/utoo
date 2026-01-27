use anyhow::{Context, Result};
use bytes::Bytes;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest::StatusCode;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use tokio::fs::File;
use tokio_retry::RetryIf;

use super::retry::build_dns_cached_client;
use super::retry::{RetryableError, create_retry_strategy};
use crate::service::pipeline::STATS;

// Global downloader client - no pool limit, concurrency controlled by OnceMap
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

// ============ Buffer Pool ============
// Reuse decompression buffers to reduce allocation overhead
// Similar to Bun's ObjectPool pattern

const BUFFER_POOL_MAX_SIZE: usize = 8;
const BUFFER_POOL_MIN_CAPACITY: usize = 2 * 1024 * 1024; // 2MB minimum

/// Global buffer pool for decompression
static BUFFER_POOL: Lazy<Mutex<Vec<Vec<u8>>>> =
    Lazy::new(|| Mutex::new(Vec::with_capacity(BUFFER_POOL_MAX_SIZE)));

/// Get a buffer from the pool or create a new one
fn acquire_buffer(required_capacity: usize) -> Vec<u8> {
    let capacity = required_capacity.max(BUFFER_POOL_MIN_CAPACITY);

    // Try to get a buffer from the pool
    if let Ok(mut pool) = BUFFER_POOL.lock() {
        // Find a buffer with sufficient capacity
        if let Some(idx) = pool.iter().position(|b| b.capacity() >= capacity) {
            let mut buf = pool.swap_remove(idx);
            buf.clear();
            tracing::trace!(
                "buffer pool: reused (capacity={}, pool_size={})",
                buf.capacity(),
                pool.len()
            );
            return buf;
        }
    }

    // No suitable buffer found, create a new one
    tracing::trace!("buffer pool: new allocation (capacity={})", capacity);
    Vec::with_capacity(capacity)
}

/// Return a buffer to the pool for reuse
fn release_buffer(mut buf: Vec<u8>) {
    // Only keep buffers that are reasonably sized
    if buf.capacity() < BUFFER_POOL_MIN_CAPACITY || buf.capacity() > 64 * 1024 * 1024 {
        tracing::trace!(
            "buffer pool: dropped (capacity={}, too small or too large)",
            buf.capacity()
        );
        return;
    }

    buf.clear();

    if let Ok(mut pool) = BUFFER_POOL.lock() {
        if pool.len() < BUFFER_POOL_MAX_SIZE {
            tracing::trace!(
                "buffer pool: returned (capacity={}, pool_size={})",
                buf.capacity(),
                pool.len() + 1
            );
            pool.push(buf);
        } else {
            tracing::trace!("buffer pool: dropped (pool full)");
        }
    }
}

/// Download tarball bytes only (network phase).
/// Returns the downloaded bytes for extraction.
pub async fn download_bytes(url: &str) -> Result<Bytes> {
    let retry_count = std::sync::atomic::AtomicU32::new(0);
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let attempt = retry_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

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
                    STATS.downloading.fetch_add(1, Ordering::Relaxed);
                    let network_start = std::time::Instant::now();
                    let bytes = response.bytes().await.map_err(|e| {
                        STATS.downloading.fetch_sub(1, Ordering::Relaxed);
                        tracing::warn!(
                            "Retry {}/10 - Stream error: {}, url: {}",
                            attempt + 1,
                            e,
                            url
                        );
                        RetryableError::Temporary(format!("Stream error: {e}"))
                    })?;
                    STATS.add_network_time(network_start.elapsed().as_micros() as u64);
                    STATS.add_bytes_downloaded(bytes.len() as u64);
                    STATS.downloading.fetch_sub(1, Ordering::Relaxed);
                    STATS.downloaded.fetch_add(1, Ordering::Relaxed);
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

/// Extract and write tarball to destination (CPU + IO phase, no network).
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
    if !(16..=512 * 1024 * 1024).contains(&size) {
        gzip_data.len() * 10
    } else {
        size
    }
}

// Extract tarball using libdeflate for better performance
async fn extract_tarball(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    // 1. Decompress and parse tar in a single blocking task (with buffer pool)
    STATS.extracting.fetch_add(1, Ordering::Relaxed);
    let decompress_start = std::time::Instant::now();
    let estimated_size = estimate_uncompressed_size(&gzip_bytes);
    let gzip_len = gzip_bytes.len();
    let dest_owned = dest.to_path_buf();

    let entries: Vec<ExtractedEntry> = tokio::task::spawn_blocking(move || -> Result<Vec<_>> {
        use std::io::Read;

        // Acquire buffer from pool
        let mut output = acquire_buffer(estimated_size);
        // SAFETY: libdeflater will write to the buffer, we don't need to initialize
        if output.capacity() < estimated_size {
            output.reserve(estimated_size - output.capacity());
        }
        unsafe { output.set_len(estimated_size) };

        let mut decompressor = libdeflater::Decompressor::new();

        let actual_size = match decompressor.gzip_decompress(&gzip_bytes, &mut output) {
            Ok(size) => {
                tracing::trace!(
                    "decompress: gzip={}, estimated={}, actual={}",
                    gzip_len,
                    estimated_size,
                    size
                );
                size
            }
            Err(libdeflater::DecompressionError::InsufficientSpace) => {
                // Buffer too small, retry with larger buffer
                tracing::debug!(
                    "decompress retry: gzip={}, estimated={} (insufficient)",
                    gzip_len,
                    estimated_size
                );
                let new_size = estimated_size * 4;
                output.reserve(new_size - output.len());
                unsafe { output.set_len(new_size) };
                decompressor
                    .gzip_decompress(&gzip_bytes, &mut output)
                    .with_context(|| "gzip decompression failed")?
            }
            Err(e) => {
                release_buffer(output);
                return Err(anyhow::anyhow!("gzip decompression failed: {}", e));
            }
        };
        output.truncate(actual_size);

        // Parse tar entries
        let cursor = std::io::Cursor::new(&output[..]);
        let mut archive = tar::Archive::new(cursor);
        let mut entries = Vec::new();

        for entry_result in archive.entries()? {
            let mut entry = entry_result.with_context(|| "Failed to read tar entry")?;
            let path = entry
                .path()
                .with_context(|| "Failed to get entry path")?
                .into_owned();
            let full_path = dest_owned.join(&path);

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

        // Release buffer back to pool
        release_buffer(output);

        Ok(entries)
    })
    .await
    .with_context(|| "Decompression/extraction task panicked")??;

    // Record decompress time
    let decompress_time = decompress_start.elapsed();
    STATS.add_decompress_time(decompress_time.as_micros() as u64);

    // 4. Write files using rayon for parallelism within spawn_blocking
    let write_start = std::time::Instant::now();
    let total_bytes: u64 = entries.iter().map(|e| e.content.len() as u64).sum();

    tokio::task::spawn_blocking(move || -> Result<()> {
        use rayon::prelude::*;
        use std::collections::HashSet;
        use std::fs;
        use std::io::Write;

        // First, create all directories (sequential to avoid race conditions)
        let mut created_dirs = HashSet::new();
        for entry in entries.iter() {
            if let Some(parent) = entry.path.parent() {
                if !created_dirs.contains(parent) {
                    fs::create_dir_all(parent).ok();
                    created_dirs.insert(parent.to_path_buf());
                }
            }
        }

        // Then write files in parallel using rayon
        entries.par_iter().try_for_each(|entry| -> Result<()> {
            let mut file = fs::File::create(&entry.path)
                .with_context(|| format!("Failed to create: {}", entry.path.display()))?;
            file.write_all(&entry.content)
                .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = fs::Permissions::from_mode(entry.mode);
                fs::set_permissions(&entry.path, perms).ok();
            }
            Ok(())
        })
    })
    .await
    .with_context(|| "Write task panicked")??;

    // Record write time and bytes
    let write_time = write_start.elapsed();
    STATS.add_write_time(write_time.as_micros() as u64);
    STATS.add_bytes_written(total_bytes);

    // Per-package timing breakdown for analysis
    let decompress_ms = decompress_time.as_millis();
    let write_ms = write_time.as_millis();
    tracing::debug!(
        "Package timing: decomp={}ms, write={}ms, path={}",
        decompress_ms,
        write_ms,
        dest.display()
    );

    // Set directory permissions and create resolution marker
    set_dir_permissions(dest).await?;
    File::create(&dest.join("_resolved"))
        .await
        .with_context(|| format!("Failed to create resolution marker in: {}", dest.display()))?;

    // Track extraction complete
    STATS.extracting.fetch_sub(1, Ordering::Relaxed);
    STATS.extracted.fetch_add(1, Ordering::Relaxed);

    Ok(())
}

/// Set directory permissions (cross-platform)
#[cfg(unix)]
async fn set_dir_permissions(path: &Path) -> Result<()> {
    use std::fs::Permissions;
    let permissions = Permissions::from_mode(0o755);
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
    path: std::path::PathBuf,
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
                let bytes = download_bytes(&url).await.unwrap();
                extract_and_write(bytes, &dest).await.unwrap();
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
