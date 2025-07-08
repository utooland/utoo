use anyhow::{Context, Result};
use async_compression::tokio::bufread::GzipDecoder;
use flate2::read::GzDecoder;
use reqwest::StatusCode;
use std::{fs::Permissions, os::unix::fs::PermissionsExt, path::Path};
use tar::Archive as TarArchive;
use tokio::{
    fs::{File, set_permissions},
    io::BufReader,
};
use tokio_retry::RetryIf;
use tokio_tar::Archive;

use std::fs;

use super::{logger::log_verbose, retry::create_retry_strategy};

// defined a custom error type to differentiate between retryable and non-retryable errors
#[derive(Debug)]
enum DownloadError {
    Permanent(String), // Cannot retry 404
    Temporary(String), // Can retry 500
}

impl std::error::Error for DownloadError {}
impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Permanent(e) => write!(f, "{e}"),
            DownloadError::Temporary(e) => write!(f, "{e}"),
        }
    }
}

pub async fn download(url: &str, dest: &Path) -> Result<()> {
    let start = std::time::Instant::now();
    RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let response = reqwest::get(url)
                .await
                .map_err(|e| DownloadError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    let bytes = response.bytes().await.map_err(|e| {
                        DownloadError::Temporary(format!("Failed to read response: {e}"))
                    })?;
                    if let Err(e) = try_unpack(&bytes, dest).await {
                        log_verbose(&format!("Unpacking failed {}: {}", dest.display(), e));
                        return Err(DownloadError::Temporary(e.to_string()));
                    }
                    Ok(())
                }
                StatusCode::NOT_FOUND => {
                    log_verbose(&format!("URL not found {url}"));
                    Err(DownloadError::Permanent(format!("URL not found {url}")))
                }
                status => {
                    log_verbose(&format!("Error: {status}, retrying"));
                    Err(DownloadError::Temporary(format!("HTTP error: {status}")))
                }
            }
        },
        |e: &DownloadError| matches!(e, DownloadError::Temporary(_)),
    )
    .await
    .context("Download failed after retries")?;

    let duration = start.elapsed();
    log_verbose(&format!("Download task took: {duration:?}, url: {url:?}"));
    Ok(())
}

async fn try_unpack(bytes: &[u8], dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dest)?;

    let tar_tgz = GzipDecoder::new(BufReader::new(bytes));
    let mut archive = Archive::new(tar_tgz);

    if (archive.unpack(dest).await).is_err() {
        let tar_gz = GzDecoder::new(bytes);
        let mut archive = TarArchive::new(tar_gz);

        for entry in archive.entries()? {
            let mut file = entry.map_err(|e| format!("Failed to read file entry: {e}"))?;
            let path = file
                .path()
                .map_err(|e| format!("Failed to parse file path: {e}"))?
                .into_owned();
            let full_path = dest.join(&path);

            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("Failed to create directory {}: {}", parent.display(), e)
                })?;
            }

            file.unpack(&full_path)
                .map_err(|e| format!("Failed to unpack file {}: {}", path.display(), e))?;

            let permissions = if full_path.is_dir() { 0o755 } else { 0o644 };

            fs::set_permissions(&full_path, fs::Permissions::from_mode(permissions))
                .map_err(|e| format!("Failed to set permissions {}: {}", path.display(), e))?;
        }
    }

    set_permissions(&dest, Permissions::from_mode(0o755)).await?;
    File::create(&dest.join("_resolved")).await?;
    Ok(())
}
