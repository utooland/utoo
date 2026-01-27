use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use reqwest::{Client, Response};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use tokio::fs::File;
use tokio_retry::RetryIf;

// Threshold for file preallocation (only worthwhile for large files)
#[cfg(target_os = "linux")]
const PREALLOCATE_THRESHOLD: usize = 1_000_000; // 1MB

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

/// Download and extract a tarball to the destination directory.
/// Deduplication is handled by the caller via OnceMap.
/// For warm cache scenarios, checks _resolved flag to skip already-downloaded packages.
pub async fn download(url: &str, dest: &Path) -> Result<()> {
    let start = std::time::Instant::now();

    // Check if already resolved (warm cache scenario)
    let resolved_path = dest.join("_resolved");
    if crate::fs::try_exists(&resolved_path).await? {
        tracing::debug!("Download skipped, already resolved: {}", dest.display());
        return Ok(());
    }

    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let response = DOWNLOADER_CLIENT
                .get(url)
                .send()
                .await
                .with_context(|| format!("Failed to send HTTP request to {url}"))
                .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    if let Err(e) = try_unpack_stream_direct(response, dest).await {
                        tracing::debug!("Stream unpacking failed {}: {:#}", dest.display(), e);
                        return Err(RetryableError::Temporary(format!(
                            "Network error during streaming: {e:#}"
                        )));
                    }
                    Ok(())
                }
                StatusCode::NOT_FOUND => {
                    tracing::debug!("URL not found {url}");
                    Err(RetryableError::Permanent(format!("URL not found {url}")))
                }
                status => {
                    tracing::debug!("Error: {status}, url: {url}, retrying");
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {status}, url: {url}"
                    )))
                }
            }
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await
    .context("Download failed after retries")?;

    let duration = start.elapsed();
    tracing::debug!("Download task took: {duration:?}, url: {url:?}");
    Ok(())
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

// Batch unpacking using libdeflate for better performance
async fn try_unpack_stream_direct(response: Response, dest: &Path) -> Result<()> {
    use dashmap::DashSet;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    crate::fs::create_dir_all(dest)
        .await
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    // 1. Download complete tarball to memory
    STATS.downloading.fetch_add(1, Ordering::Relaxed);
    let gzip_bytes = response
        .bytes()
        .await
        .with_context(|| "Failed to download tarball")?;
    STATS.downloading.fetch_sub(1, Ordering::Relaxed);
    STATS.downloaded.fetch_add(1, Ordering::Relaxed);

    // 2. Decompress and parse tar in a single blocking task (with buffer pool)
    STATS.extracting.fetch_add(1, Ordering::Relaxed);
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

    // 4. Write files concurrently
    let semaphore = Arc::new(Semaphore::new(16));
    let created_dirs = Arc::new(DashSet::<std::path::PathBuf>::new());
    let mut write_tasks = Vec::with_capacity(entries.len());

    for entry in entries {
        let semaphore = Arc::clone(&semaphore);
        let created_dirs = Arc::clone(&created_dirs);

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            // Ensure parent directory exists using cache
            if let Some(parent) = entry.path.parent() {
                let parent_path = parent.to_path_buf();
                if !created_dirs.contains(&parent_path) {
                    crate::fs::create_dir_all(&parent_path).await.ok();
                    created_dirs.insert(parent_path);
                }
            }

            // Write file content (with preallocation on Linux for large files)
            write_file_optimized(&entry.path, &entry.content)
                .await
                .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

            // Set file permissions (Unix only)
            set_file_permissions(&entry.path, entry.mode).await?;

            Ok::<(), anyhow::Error>(())
        });

        write_tasks.push(task);
    }

    // Wait for all writes to complete
    for task in write_tasks {
        task.await??;
    }

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

/// Write file with optional preallocation (Linux only for large files)
/// On Linux, fallocate() can improve write performance for files >1MB
/// by pre-reserving disk space and reducing fragmentation.
#[cfg(target_os = "linux")]
async fn write_file_optimized(path: &Path, content: &[u8]) -> Result<()> {
    use std::os::unix::io::AsRawFd;
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::File::create(path).await?;

    // Preallocate for large files (>1MB)
    if content.len() > PREALLOCATE_THRESHOLD {
        let fd = file.as_raw_fd();
        // fallocate(fd, mode=0, offset=0, len)
        // mode=0 means default allocation (allocate disk space)
        unsafe {
            libc::fallocate(fd, 0, 0, content.len() as libc::off_t);
        }
        tracing::trace!(
            "preallocate: {} bytes for {}",
            content.len(),
            path.display()
        );
    }

    file.write_all(content).await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
async fn write_file_optimized(path: &Path, content: &[u8]) -> Result<()> {
    crate::fs::write(path, content).await?;
    Ok(())
}

/// Set file permissions (cross-platform)
#[cfg(unix)]
async fn set_file_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::fs::Permissions;
    let permissions = Permissions::from_mode(mode);
    crate::fs::set_permissions(path, permissions)
        .await
        .with_context(|| format!("Failed to set permissions for: {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_file_permissions(_path: &Path, _mode: u32) -> Result<()> {
    // Windows doesn't need Unix-style permissions
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
