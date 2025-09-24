use anyhow::{Context, Result};
use async_compression::tokio::bufread::GzipDecoder;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest::StatusCode;
use std::{fs::Permissions, os::unix::fs::PermissionsExt, path::Path};
use tokio::{
    fs::{File, set_permissions},
    io::BufReader,
};
use tokio_retry::RetryIf;
use tokio_tar::Archive;

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
    if resolved_path.exists() {
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
                .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    let bytes = response.bytes().await.map_err(|e| {
                        RetryableError::Temporary(format!("Failed to read response: {e}"))
                    })?;
                    if let Err(e) = try_unpack(&bytes, dest).await {
                        log_verbose(&format!("Unpacking failed {}: {}", dest.display(), e));
                        return Err(RetryableError::Temporary(e.to_string()));
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

async fn try_unpack(bytes: &[u8], dest: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dest).await?;

    let tar_tgz = GzipDecoder::new(BufReader::new(bytes));
    let mut archive = Archive::new(tar_tgz);

    archive
        .unpack(dest)
        .await
        .context("Failed to unpack tar.gz archive")?;

    set_permissions(dest, Permissions::from_mode(0o755)).await?;

    File::create(&dest.join("_resolved")).await?;

    Ok(())
}
