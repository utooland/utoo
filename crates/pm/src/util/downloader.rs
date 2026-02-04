use anyhow::{Context, Result};
use async_compression::tokio::bufread::GzipDecoder;
use futures::StreamExt;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use reqwest::StatusCode;
use reqwest::{Client, Response};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_retry::RetryIf;
use tokio_tar::Archive;
use tokio_util::io::StreamReader;

use super::oncemap::OnceMap;
use super::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};

// Global downloader client - no pool limit, concurrency controlled externally
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

// OnceMap to ensure each (url, dest) pair is only downloaded once
static DOWNLOAD_ONCE: Lazy<OnceMap<(String, PathBuf), ()>> = Lazy::new(OnceMap::new);

/// Download and extract a tarball to the destination directory.
///
/// Uses OnceMap to ensure each (url, dest) pair is only downloaded once,
/// even when called concurrently from multiple tasks.
pub async fn download(url: &str, dest: &Path) -> Result<()> {
    let key = (url.to_string(), dest.to_path_buf());

    DOWNLOAD_ONCE
        .get_or_init(key, || async {
            download_and_extract(url, dest).await.ok()?;
            Some(())
        })
        .await
        .map(|_| ())
        .ok_or_else(|| anyhow::anyhow!("Download failed: {}", url))
}

async fn download_and_extract(url: &str, dest: &Path) -> Result<()> {
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

    Ok(())
}

// Stream-based unpacking directly from HTTP Response
async fn try_unpack_stream_direct(response: Response, dest: &Path) -> Result<()> {
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

    // Stage 1: Streaming tar extraction
    let extraction_task = {
        let entry_tx = entry_tx.clone();
        let dest = dest.clone();

        tokio::spawn(async move {
            // Create streaming gzip decoder
            let gzip_decoder = GzipDecoder::new(stream_reader);
            let mut tar_archive = Archive::new(gzip_decoder);
            let mut entries = tar_archive.entries()?;

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
                    entry
                        .read_to_end(&mut content)
                        .await
                        .with_context(|| format!("Failed to read tar entry: {}", path.display()))?;

                    // Extract file permission mode
                    let mode = entry.header().mode().unwrap_or(0o644);

                    let extracted_entry = ExtractedEntry {
                        path: full_path,
                        content,
                        mode,
                    };

                    if entry_tx.send(extracted_entry).await.is_err() {
                        break;
                    }
                }
            }

            Ok::<(), anyhow::Error>(())
        })
    };

    // Stage 2: Collect all entries, then rayon parallel write
    let file_writing_task = {
        tokio::spawn(async move {
            use std::collections::HashSet;

            // Collect all entries from channel
            let mut entries = Vec::new();
            while let Some(entry) = entry_rx.recv().await {
                entries.push(entry);
            }

            // Write all files using spawn_blocking + rayon
            tokio::task::spawn_blocking(move || {
                // Create all parent directories first (sequential)
                let mut created_dirs = HashSet::new();
                for entry in &entries {
                    if let Some(parent) = entry.path.parent()
                        && created_dirs.insert(parent.to_path_buf())
                    {
                        std::fs::create_dir_all(parent).ok();
                    }
                }

                // Write files in parallel using rayon
                entries.par_iter().try_for_each(|entry| {
                    std::fs::write(&entry.path, &entry.content).with_context(|| {
                        format!("Failed to write file: {}", entry.path.display())
                    })?;
                    set_file_permissions_sync(&entry.path, entry.mode)?;
                    Ok::<(), anyhow::Error>(())
                })
            })
            .await?
        })
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

/// Set file permissions synchronously (cross-platform)
#[cfg(unix)]
fn set_file_permissions_sync(path: &Path, mode: u32) -> Result<()> {
    use std::fs::Permissions;
    let permissions = Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)
        .with_context(|| format!("Failed to set permissions for: {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions_sync(_path: &Path, _mode: u32) -> Result<()> {
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
    path: PathBuf,
    content: Vec<u8>,
    mode: u32,
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
