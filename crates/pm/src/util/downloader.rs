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

// Stream-based unpacking directly from HTTP Response
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
                }
            }

            Ok::<(), anyhow::Error>(())
        })
    };

    // Stage 2: Concurrent file writing with cached directory creation
    let file_writing_task = {
        tokio::spawn(async move {
            use dashmap::DashSet;

            let semaphore = Arc::new(Semaphore::new(16));
            let created_dirs = Arc::new(DashSet::<std::path::PathBuf>::new());
            let mut write_tasks = Vec::new();
            let mut batch_size = 0;
            let mut total_bytes = 0;
            const MAX_BATCH_SIZE: usize = 100;
            const MAX_BATCH_BYTES: usize = 50 * 1024 * 1024; // 50MB

            while let Some(entry) = entry_rx.recv().await {
                let semaphore = Arc::clone(&semaphore);
                let created_dirs = Arc::clone(&created_dirs);
                batch_size += 1;
                total_bytes += entry.size;

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
                                .context(format!("Parent directory: {}", parent_path.display())));
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
                    total_bytes = 0;
                }
            }

            // Wait for remaining tasks
            for task in write_tasks {
                task.await??;
            }

            Ok::<(), anyhow::Error>(())
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
    use std::env;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tar::Builder;
    use tempfile::TempDir;
    use tokio::task;
    use tokio::time::sleep;

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

    fn create_package_tar_gz() -> (Vec<u8>, String) {
        let package_json = r#"{"name":"race-pkg","version":"1.0.0"}"#.to_string();
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);
            append_tar_file(&mut tar, "package/package.json", package_json.as_bytes());

            for index in 0..128 {
                let content = format!(
                    "file-{index}\n{}",
                    "0123456789abcdefghijklmnopqrstuvwxyz".repeat(256)
                );
                append_tar_file(
                    &mut tar,
                    &format!("package/lib/file-{index}.txt"),
                    content.as_bytes(),
                );
            }

            tar.finish().unwrap();
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_data).unwrap();
        (encoder.finish().unwrap(), package_json)
    }

    fn create_large_package_json_tar_gz() -> (Vec<u8>, usize) {
        let package_json = format!(
            r#"{{"name":"race-pkg","version":"1.0.0","padding":"{}"}}"#,
            "x".repeat(64 * 1024 * 1024)
        );
        let package_json_len = package_json.len();
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);
            append_tar_file(&mut tar, "package/package.json", package_json.as_bytes());
            tar.finish().unwrap();
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&tar_data).unwrap();
        (encoder.finish().unwrap(), package_json_len)
    }

    fn append_tar_file(tar: &mut Builder<&mut Vec<u8>>, path: &str, content: &[u8]) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, content).unwrap();
    }

    fn spawn_gated_download_helper(
        test_binary: &Path,
        url: &str,
        dest: &Path,
        ready_dir: &Path,
        start_file: &Path,
    ) -> Child {
        Command::new(test_binary)
            .arg("util::downloader::tests::download_process_helper")
            .arg("--ignored")
            .arg("--nocapture")
            .env("UTOO_DOWNLOAD_HELPER", "1")
            .env("UTOO_DOWNLOAD_HELPER_URL", url)
            .env("UTOO_DOWNLOAD_HELPER_DEST", dest)
            .env("UTOO_DOWNLOAD_HELPER_READY_DIR", ready_dir)
            .env("UTOO_DOWNLOAD_HELPER_START_FILE", start_file)
            .spawn()
            .unwrap()
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

    #[tokio::test]
    #[ignore]
    async fn test_download_shared_dest_across_processes() {
        let (tar_gz, package_json) = create_package_tar_gz();
        let body = Arc::new(tar_gz);
        let mut server = Server::new_async().await;
        let child_count = 8;
        let _m = server
            .mock("GET", "/pkg.tgz")
            .with_status(200)
            .with_header("content-type", "application/gzip")
            .with_chunked_body(move |writer| {
                std::thread::sleep(Duration::from_millis(200));
                for chunk in body.chunks(1024) {
                    writer.write_all(chunk)?;
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(())
            })
            .expect_at_least(2)
            .expect_at_most(child_count)
            .create_async()
            .await;

        let url = format!("{}/pkg.tgz", server.url());
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");
        let ready_dir = temp_dir.path().join("ready");
        let start_file = temp_dir.path().join("start");
        crate::fs::create_dir_all(&ready_dir).await.unwrap();
        let test_binary = env::current_exe().unwrap();
        let mut children = Vec::new();

        for _ in 0..child_count {
            children.push(spawn_gated_download_helper(
                &test_binary,
                &url,
                &dest,
                &ready_dir,
                &start_file,
            ));
        }

        wait_for_ready_files(&ready_dir, child_count, Duration::from_secs(5)).await;
        crate::fs::write(&start_file, "").await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut failures = Vec::new();
        for mut child in children {
            loop {
                match child.try_wait().unwrap() {
                    Some(status) => {
                        if !status.success() {
                            failures.push(status.to_string());
                        }
                        break;
                    }
                    None if Instant::now() >= deadline => {
                        let _ = child.kill();
                        failures.push("timed out".to_string());
                        break;
                    }
                    None => sleep(Duration::from_millis(25)).await,
                }
            }
        }

        assert!(failures.is_empty(), "download helpers failed: {failures:?}");
        assert!(dest.join("_resolved").exists());

        let content = crate::fs::read_to_string(dest.join("package/package.json"))
            .await
            .unwrap();
        assert_eq!(content, package_json);

        for index in [0, 31, 63, 127] {
            let path = dest.join(format!("package/lib/file-{index}.txt"));
            let metadata = crate::fs::metadata(&path).await.unwrap();
            assert!(metadata.len() > 0, "{} was empty", path.display());
        }

        _m.assert();
    }

    #[tokio::test]
    #[ignore]
    async fn repro_download_can_leave_zero_file_if_racing_process_dies_after_resolved() {
        let (tar_gz, expected_package_json_len) = create_large_package_json_tar_gz();
        let body = Arc::new(tar_gz);
        let request_index = Arc::new(AtomicUsize::new(0));
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/pkg.tgz")
            .with_status(200)
            .with_header("content-type", "application/gzip")
            .with_chunked_body(move |writer| {
                let index = request_index.fetch_add(1, Ordering::SeqCst);
                if index == 1 {
                    std::thread::sleep(Duration::from_millis(500));
                }

                for chunk in body.chunks(64 * 1024) {
                    writer.write_all(chunk)?;
                }
                Ok(())
            })
            .expect(2)
            .create_async()
            .await;

        let url = format!("{}/pkg.tgz", server.url());
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("pkg");
        let package_json_path = dest.join("package/package.json");
        let ready_dir = temp_dir.path().join("ready");
        let start_file = temp_dir.path().join("start");
        crate::fs::create_dir_all(&ready_dir).await.unwrap();
        let test_binary = env::current_exe().unwrap();
        let children = vec![
            spawn_gated_download_helper(&test_binary, &url, &dest, &ready_dir, &start_file),
            spawn_gated_download_helper(&test_binary, &url, &dest, &ready_dir, &start_file),
        ];

        wait_for_ready_files(&ready_dir, 2, Duration::from_secs(5)).await;
        crate::fs::write(&start_file, "").await.unwrap();

        wait_for_path(&dest.join("_resolved"), Duration::from_secs(5)).await;
        wait_for_zero_len(&package_json_path, Duration::from_secs(15)).await;

        for child in &children {
            if child.id() != 0 {
                let _ = Command::new("kill")
                    .arg("-KILL")
                    .arg(child.id().to_string())
                    .status();
            }
        }

        for mut child in children {
            let _ = child.wait();
        }

        assert!(dest.join("_resolved").exists());
        assert_eq!(
            crate::fs::metadata(&package_json_path).await.unwrap().len(),
            0,
            "racing writer finished instead of being killed after truncating package.json; expected full size was {expected_package_json_len}"
        );

        _m.assert();
    }

    async fn wait_for_path(path: &Path, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if path.exists() {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {}", path.display());
    }

    async fn wait_for_ready_files(path: &Path, count: usize, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let ready_count = std::fs::read_dir(path)
                .map(|entries| entries.count())
                .unwrap_or(0);
            if ready_count >= count {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "timed out waiting for {count} ready files in {}",
            path.display()
        );
    }

    async fn wait_for_zero_len(path: &Path, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(metadata) = crate::fs::metadata(path).await
                && metadata.len() == 0
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for zero-length {}", path.display());
    }

    #[test]
    #[ignore]
    fn download_process_helper() {
        if env::var("UTOO_DOWNLOAD_HELPER").ok().as_deref() != Some("1") {
            return;
        }

        let url = env::var("UTOO_DOWNLOAD_HELPER_URL").unwrap();
        let dest = PathBuf::from(env::var("UTOO_DOWNLOAD_HELPER_DEST").unwrap());
        if let Ok(ready_dir) = env::var("UTOO_DOWNLOAD_HELPER_READY_DIR") {
            std::fs::create_dir_all(&ready_dir).unwrap();
            std::fs::write(
                PathBuf::from(ready_dir).join(std::process::id().to_string()),
                b"ready",
            )
            .unwrap();

            let start_file = PathBuf::from(env::var("UTOO_DOWNLOAD_HELPER_START_FILE").unwrap());
            while !start_file.exists() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            download(&url, &dest).await.unwrap();
        });
    }
}
