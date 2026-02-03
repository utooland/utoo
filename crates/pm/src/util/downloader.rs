use anyhow::{Context, Result};
use async_compression::tokio::bufread::GzipDecoder;
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use reqwest::{Client, Response};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::{fs::File, io::AsyncReadExt};
use tokio_retry::RetryIf;
use tokio_tar::Archive;
use tokio_util::io::StreamReader;
use tracing::{Instrument, instrument};

use super::retry::build_dns_cached_client;
use super::retry::{RetryableError, create_retry_strategy};

use dashmap::DashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::Mutex;

// Global downloader client - no pool limit, concurrency controlled by semaphore
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

static DOWNLOAD_LOCKS: Lazy<DashMap<u64, Arc<Mutex<()>>>> = Lazy::new(DashMap::new);

fn lock_key(url: &str, dest: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    dest.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}

/// Download and extract a tarball from URL to destination directory.
/// Uses streaming decompression pipeline for memory efficiency.
#[instrument(name = "download", skip_all, fields(url = %url))]
pub async fn download(url: &str, dest: &Path) -> Result<()> {
    let start = std::time::Instant::now();
    let key = lock_key(url, dest);
    let lock = DOWNLOAD_LOCKS
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;

    let resolved_path = dest.join("_resolved");
    if crate::fs::try_exists(&resolved_path).await? {
        tracing::debug!("Download skipped, already resolved: {}", dest.display());
        return Ok(());
    }

    RetryIf::spawn(
        create_retry_strategy(),
        || {
            let http_span = tracing::trace_span!("http_request", url = %url);
            async {
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
            }
            .instrument(http_span)
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await
    .context("Download failed after retries")?;

    let duration = start.elapsed();
    tracing::debug!("Download task took: {duration:?}, url: {url:?}");
    Ok(())
}

/// Stream-based unpacking directly from HTTP Response.
/// Uses a two-stage pipeline: gzip decode + tar extract -> concurrent file write.
#[instrument(name = "unpack_stream", skip_all)]
async fn try_unpack_stream_direct(response: Response, dest: &Path) -> Result<()> {
    use std::sync::Arc;
    use tokio::sync::{Semaphore, mpsc};

    crate::fs::create_dir_all(dest)
        .await
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    // Convert HTTP response stream to AsyncRead for streaming processing
    let stream = response
        .bytes_stream()
        .map(|result| result.map_err(std::io::Error::other));
    let stream_reader = StreamReader::new(stream);

    // Create pipeline processing channels
    let (entry_tx, mut entry_rx) = mpsc::channel::<ExtractedEntry>(500);

    let dest = dest.to_path_buf();

    // Stage 1: Streaming tar extraction (gzip decode + tar extract)
    let extraction_task = {
        let entry_tx = entry_tx.clone();
        let dest = dest.clone();

        tokio::spawn(
            async move {
                // Create streaming gzip decoder
                let gzip_decoder = GzipDecoder::new(stream_reader);
                let mut tar_archive = Archive::new(gzip_decoder);
                let mut entries = tar_archive.entries()?;
                let mut file_count = 0u32;

                while let Some(entry_result) = entries.next().await {
                    let mut entry = entry_result.with_context(|| "Failed to read tar entry")?;
                    let path = entry
                        .path()
                        .with_context(|| "Failed to get entry path")?
                        .into_owned();
                    let full_path = dest.join(&path);
                    let is_dir = entry.header().entry_type().is_dir();

                    // Only process files, skip directories (they'll be created when writing files)
                    if !is_dir {
                        // Stream file content
                        let mut content = Vec::new();
                        entry.read_to_end(&mut content).await.with_context(|| {
                            format!("Failed to read tar entry: {}", path.display())
                        })?;

                        // Extract file permission mode
                        let mode = entry.header().mode().unwrap_or(0o644);

                        let size = content.len();
                        let extracted_entry = ExtractedEntry {
                            path: full_path,
                            content,
                            size,
                            mode,
                        };

                        if entry_tx.send(extracted_entry).await.is_err() {
                            break;
                        }
                        file_count += 1;
                    }
                }

                tracing::trace!(file_count, "tar extraction completed");
                Ok::<(), anyhow::Error>(())
            }
            .instrument(tracing::trace_span!("tar_extract")),
        )
    };

    // Stage 2: Concurrent file writing with cached directory creation
    let file_writing_task = {
        tokio::spawn(
            async move {
                use dashmap::DashSet;

                let semaphore = Arc::new(Semaphore::new(16));
                let created_dirs = Arc::new(DashSet::<std::path::PathBuf>::new());
                let mut write_tasks = Vec::new();
                let mut batch_size = 0;
                let mut total_bytes: usize = 0;
                let mut total_files: u32 = 0;
                const MAX_BATCH_SIZE: usize = 100;
                const MAX_BATCH_BYTES: usize = 50 * 1024 * 1024; // 50MB

                while let Some(entry) = entry_rx.recv().await {
                    let semaphore = Arc::clone(&semaphore);
                    let created_dirs = Arc::clone(&created_dirs);
                    batch_size += 1;
                    total_bytes += entry.size;
                    total_files += 1;

                    let task = tokio::spawn(async move {
                        let _permit = semaphore.acquire().await.unwrap();

                        // Ensure parent directory exists using cache
                        if let Some(parent) = entry.path.parent() {
                            let parent_path = parent.to_path_buf();

                            // Check cache first to avoid duplicate directory creation
                            if !created_dirs.contains(&parent_path) {
                                if let Err(e) = crate::fs::create_dir_all(&parent_path).await {
                                    tracing::debug!(
                                        "Failed to create parent dir {}: {}",
                                        parent_path.display(),
                                        e
                                    );
                                    return Err(anyhow::anyhow!(
                                        "Failed to create parent directory: {e}"
                                    )
                                    .context(format!(
                                        "Parent directory: {}",
                                        parent_path.display()
                                    )));
                                }

                                created_dirs.insert(parent_path);
                            }
                        }

                        // Write file content
                        if let Err(e) = crate::fs::write(&entry.path, &entry.content).await {
                            tracing::debug!("Failed to write file {}: {}", entry.path.display(), e);
                            return Err(anyhow::anyhow!("Write failed: {e}")
                                .context(format!("File path: {}", entry.path.display())));
                        }

                        // Set original file permissions from tar entry (Unix only)
                        set_file_permissions(&entry.path, entry.mode).await?;

                        Ok::<(), anyhow::Error>(())
                    });

                    write_tasks.push(task);

                    // Process in batches to manage memory and concurrency
                    if batch_size >= MAX_BATCH_SIZE
                        || total_bytes >= MAX_BATCH_BYTES
                        || entry_rx.is_empty()
                    {
                        for task in write_tasks.drain(..) {
                            task.await??;
                        }
                        batch_size = 0;
                    }
                }

                // Wait for remaining tasks
                for task in write_tasks {
                    task.await??;
                }

                tracing::trace!(total_files, total_bytes, "file writing completed");
                Ok::<(), anyhow::Error>(())
            }
            .instrument(tracing::trace_span!("file_write_batch")),
        )
    };

    // Close sender channel
    drop(entry_tx);

    // Wait for both stages to complete
    let (extract_result, write_result) = tokio::try_join!(extraction_task, file_writing_task)?;

    extract_result?;
    write_result?;

    // Set directory permissions and create resolution marker
    set_dir_permissions(&dest).await?;
    File::create(&dest.join("_resolved"))
        .await
        .with_context(|| format!("Failed to create resolution marker in: {}", dest.display()))?;

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
    size: usize,
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
