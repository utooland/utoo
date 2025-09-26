use anyhow::{Context, Result};
use async_compression::tokio::bufread::GzipDecoder;
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use reqwest::{Client, Response};
use std::collections::HashSet;
use std::{fs::Permissions, os::unix::fs::PermissionsExt, path::Path};
use tokio::fs::{File, set_permissions};
use tokio_retry::RetryIf;
use tokio_tar::Archive;
use tokio_util::io::StreamReader;

use super::retry::build_dns_cached_client;
use super::{
    logger::log_verbose,
    retry::{RetryableError, create_retry_strategy},
};

use dashmap::DashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::Mutex;

// Global downloader client with DNS cache
static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

static DOWNLOAD_LOCKS: Lazy<DashMap<u64, Arc<Mutex<()>>>> = Lazy::new(DashMap::new);

fn lock_key(url: &str, dest: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    dest.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}

pub async fn download(url: &str, dest: &Path) -> Result<()> {
    let start = std::time::Instant::now();
    let key = lock_key(url, dest);
    let lock = DOWNLOAD_LOCKS
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;

    let resolved_path = dest.join("_resolved");
    if tokio::fs::try_exists(&resolved_path).await? {
        log_verbose(&format!(
            "Download skipped, already resolved: {}",
            dest.display()
        ));
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
                        log_verbose(&format!(
                            "Stream unpacking failed {}: {}",
                            dest.display(),
                            e
                        ));
                        return Err(RetryableError::Temporary(format!(
                            "Network error during streaming: {e}"
                        )));
                    }
                    Ok(())
                }
                StatusCode::NOT_FOUND => {
                    log_verbose(&format!("URL not found {url}"));
                    Err(RetryableError::Permanent(format!("URL not found {url}")))
                }
                status => {
                    log_verbose(&format!("Error: {status}, url: {url}, retrying"));
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
    log_verbose(&format!("Download task took: {duration:?}, url: {url:?}"));
    Ok(())
}

// Stream-based unpacking directly from HTTP Response
async fn try_unpack_stream_direct(response: Response, dest: &Path) -> Result<()> {
    let mut join_set = tokio::task::JoinSet::new();
    tokio::fs::create_dir_all(dest)
        .await
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    // Convert HTTP response stream to AsyncRead for streaming processing
    let stream = response
        .bytes_stream()
        .map(|result| result.map_err(std::io::Error::other));
    let stream_reader = StreamReader::new(stream);

    let dest = dest.to_path_buf();

    let mut created_dirs = HashSet::<std::path::PathBuf>::new();

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
        let full_path = &dest.join(&path);
        let is_dir = entry.header().entry_type().is_dir();

        // Only process files, skip directories (they'll be created when writing files)
        if !is_dir {
            // Extract file permission mode
            let mode = entry.header().mode().unwrap_or(0o644);

            if let Some(parent) = full_path.parent() {
                let parent_path = parent.to_path_buf();

                // Check cache first to avoid duplicate directory creation
                if !created_dirs.contains(&parent_path) {
                    if let Err(e) = tokio::fs::create_dir_all(&parent_path).await {
                        log_verbose(&format!(
                            "Failed to create parent dir {}: {}",
                            parent_path.display(),
                            e
                        ));
                        return Err(anyhow::anyhow!("Failed to create parent directory: {}", e)
                            .context(format!("Parent directory: {}", parent_path.display())));
                    }

                    created_dirs.insert(parent_path);
                }
            }

            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&full_path)
                .await
                .context(format!("Failed to open file {}", full_path.display()))?;

            {
                let full_path = full_path.clone();
                join_set.spawn(async move {
                    if let Err(err) = tokio::io::copy(&mut entry, &mut file).await {
                        log_verbose(&format!("Failed to write file {}", full_path.display()));
                        panic!("{err}")
                    }
                });
            }

            // Set original file permissions from tar entry
            let permissions = Permissions::from_mode(mode);
            if let Err(e) = tokio::fs::set_permissions(&full_path, permissions).await {
                log_verbose(&format!(
                    "Failed to set permissions {}: {}",
                    full_path.display(),
                    e
                ));
            }
        }
    }

    join_set.join_all().await;

    // Set directory permissions and create resolution marker
    set_permissions(&dest, Permissions::from_mode(0o755))
        .await
        .with_context(|| format!("Failed to set directory permissions: {}", dest.display()))?;
    File::create(&dest.join("_resolved"))
        .await
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
        let content = tokio::fs::read_to_string(dest.join("file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");
        _m.assert();
    }
}
