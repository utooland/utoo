use anyhow::Context;
use anyhow::Result;
use dashmap::DashMap;
use glob::glob;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use tokio::sync::mpsc;

use crate::cmd::rebuild::rebuild;
use crate::helper::global_bin::get_global_bin_dir;
use crate::helper::lock::{
    Package, PackageLock, ensure_package_lock, extract_package_name, group_by_depth,
    path_to_pkg_name, prepare_global_package_json, update_package_json,
};
use crate::helper::workspace;
use crate::helper::{is_cpu_compatible, is_os_compatible};
use crate::model::package::PackageInfo;
use crate::util::cache::get_cache_dir;
use crate::util::linker::link;
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_info, log_progress, log_verbose, start_progress_bar,
};
use crate::util::save_type::{PackageAction, SaveType};

use super::binary::update_package_binary;

/// Clean up a single node_modules directory
async fn clean_node_modules_dir(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &std::collections::HashSet<String>,
) -> Result<()> {
    // clean up symlinks for npminstall
    if let Ok(mut entries) = tokio::fs::read_dir(node_modules).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_symlink() {
                clean_symlink(&path).await?;
            } else if path.is_dir() {
                clean_directory(&path).await?;
            }
        }
    }

    clean_unused_packages(node_modules, cwd, valid_packages).await?;

    Ok(())
}

/// Clean up a symlink
async fn clean_symlink(path: &Path) -> Result<()> {
    log_verbose(&format!("Removing symlink: {}", path.display()));
    if let Err(e) = tokio::fs::remove_file(path).await {
        log_verbose(&format!(
            "Failed to remove symlink {}: {}",
            path.display(),
            e
        ));
    }
    Ok(())
}

/// Clean up a directory, handling scoped packages and legacy npm install packages
async fn clean_directory(path: &Path) -> Result<()> {
    if let Some(file_name) = path.file_name()
        && let Some(name) = file_name.to_str()
    {
        if name.starts_with('@') {
            clean_scoped_package(path).await?;
        } else {
            clean_legacy_npminstall_package(path, name).await?;
        }
    }
    Ok(())
}

/// Clean up a scoped package directory
async fn clean_scoped_package(path: &Path) -> Result<()> {
    if let Ok(mut scope_entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(scope_entry)) = scope_entries.next_entry().await {
            let scope_path = scope_entry.path();
            if scope_path.is_symlink() {
                log_verbose(&format!(
                    "Removing scoped symlink: {}",
                    scope_path.display()
                ));
                if let Err(e) = tokio::fs::remove_file(&scope_path).await {
                    log_verbose(&format!(
                        "Failed to remove scoped symlink {}: {}",
                        scope_path.display(),
                        e
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Clean up a legacy npminstall package
async fn clean_legacy_npminstall_package(path: &Path, name: &str) -> Result<()> {
    let at_count = name.matches('@').count();
    if name.starts_with('_') && (at_count == 2 || at_count == 4) {
        log_verbose(&format!("Removing legacy package: {}", path.display()));
        if let Err(e) = tokio::fs::remove_dir_all(path).await {
            log_verbose(&format!(
                "Failed to remove legacy package {}: {}",
                path.display(),
                e
            ));
        }
    }
    Ok(())
}

/// Clean up unused packages in the node_modules directory
async fn clean_unused_packages(
    node_modules: &Path,
    cwd: &Path,
    valid_packages: &std::collections::HashSet<String>,
) -> Result<()> {
    // Helper function for recursive search
    fn find_and_clean<'a>(
        node_modules: &'a Path,
        cwd: &'a Path,
        valid_packages: &'a std::collections::HashSet<String>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let patterns = [
                node_modules.join("*/package.json"),
                node_modules.join("@*/*/package.json"),
            ];
            for pattern in patterns.iter() {
                let pattern_str = pattern.to_string_lossy().to_string();
                for entry in glob(&pattern_str)
                    .with_context(|| format!("Glob failed for pattern: {pattern_str}"))?
                {
                    let pkg_json_path = entry
                        .with_context(|| format!("Glob entry error for pattern: {pattern_str}"))?;
                    let pkg_dir = pkg_json_path
                        .parent()
                        .context("Failed to get parent directory of package.json")?;
                    if let Some(pkg_name) = path_to_pkg_name(&pkg_dir.to_string_lossy()) {
                        let pkg_path = pkg_dir.strip_prefix(cwd).with_context(|| {
                            format!(
                                "Failed to strip prefix {} from {}",
                                cwd.display(),
                                pkg_dir.display()
                            )
                        })?;
                        if !valid_packages.contains(pkg_path.to_string_lossy().as_ref()) {
                            log_verbose(&format!("Cleaning unused package: {pkg_name}"));
                            if let Err(e) = tokio::fs::remove_dir_all(pkg_dir).await {
                                log_verbose(&format!("Failed to remove {pkg_name}: {e}"));
                            }
                        }
                    }
                    // Recursively check nested node_modules
                    let nested_node_modules = pkg_dir.join("node_modules");
                    if nested_node_modules.exists() {
                        find_and_clean(&nested_node_modules, cwd, valid_packages).await?;
                    }
                }
            }
            Ok(())
        })
    }
    find_and_clean(node_modules, cwd, valid_packages).await?;
    Ok(())
}

async fn clean_deps(groups: &HashMap<usize, Vec<(String, Package)>>, cwd: &Path) -> Result<()> {
    let mut valid_packages = std::collections::HashSet::new();
    for packages in groups.values() {
        for (path, _) in packages {
            valid_packages.insert(path.clone());
        }
    }

    log_verbose(&format!("Valid packages: {valid_packages:?}"));

    let mut node_modules_dirs = vec![cwd.join("node_modules")];

    let workspaces = workspace::find_workspaces(cwd).await?;
    for (_, path, _) in workspaces {
        let workspace_node_modules = path.join("node_modules");
        if workspace_node_modules.exists() {
            node_modules_dirs.push(workspace_node_modules.clone());
            log_verbose(&format!(
                "add workspace node_modules: {:?}",
                workspace_node_modules.display()
            ));
        }
    }

    // cleanup unused packages in all workspace_members
    for node_modules in node_modules_dirs {
        clean_node_modules_dir(&node_modules, cwd, &valid_packages).await?;
    }

    Ok(())
}

// Unique package identifier
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct PackageKey {
    name: String,
    version: String,
    resolved_url: String,
}

// Clone target information
#[derive(Debug, Clone)]
struct CloneTarget {
    target_path: PathBuf,
    package_name: String,
}

// Download task
#[derive(Debug)]
struct DownloadTask {
    key: PackageKey,
    cache_path: PathBuf,
    has_install_script: Option<bool>,
}

// Streaming extract task - no intermediate file storage
#[derive(Debug)]
struct StreamingExtractTask {
    downloaded_data: Vec<u8>,
    cache_path: PathBuf,
    package_key: PackageKey,
    has_install_script: Option<bool>,
    clone_targets: Vec<CloneTarget>,
}

// Clone task
#[derive(Debug)]
struct CloneTask {
    cache_path: PathBuf,
    target_path: PathBuf,
    package_name: String,
}

// Simplified package completion tracker
#[derive(Debug)]
struct PackageCompletionTracker {
    package_key: PackageKey,
    cache_path: PathBuf,
    clone_targets: Vec<CloneTarget>,
    has_install_script: Option<bool>,
}

// Download deduplicator
#[derive(Debug)]
struct DownloadDeduplicator {
    in_progress: DashMap<PackageKey, Vec<CloneTarget>>,
}

#[derive(Debug)]
enum PackageStatus {
    NeedDownload,
    AlreadyCached,
    InProgress,
}

impl DownloadDeduplicator {
    fn new() -> Self {
        Self {
            in_progress: DashMap::new(),
        }
    }

    /// Check package status and register clone target
    fn check_and_register(
        &self,
        key: PackageKey,
        clone_target: CloneTarget,
        cache_flag_path: &Path,
    ) -> PackageStatus {
        // Check if cache already exists
        if cache_flag_path.exists() {
            return PackageStatus::AlreadyCached;
        }

        // Try to register to in-progress download task
        match self.in_progress.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // Download task already exists, add clone target
                entry.get_mut().push(clone_target);
                PackageStatus::InProgress
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // New download task, create clone target list
                entry.insert(vec![clone_target]);
                PackageStatus::NeedDownload
            }
        }
    }

    /// Get and remove clone target list for completed package
    fn complete_package(&self, key: &PackageKey) -> Option<Vec<CloneTarget>> {
        self.in_progress.remove(key).map(|(_, targets)| targets)
    }
}

impl PackageCompletionTracker {
    fn new(
        package_key: PackageKey,
        cache_path: PathBuf,
        clone_targets: Vec<CloneTarget>,
        has_install_script: Option<bool>,
    ) -> Self {
        Self {
            package_key,
            cache_path,
            clone_targets,
            has_install_script,
        }
    }
}

// Simple depth synchronization using global counter
static DEPTH_COMPLETION_COUNTER: AtomicUsize = AtomicUsize::new(0);

// Pre-download task management
static PRE_DOWNLOAD_SEMAPHORE: Lazy<tokio::sync::Semaphore> = Lazy::new(|| {
    tokio::sync::Semaphore::new(4) // 限制4个并发预下载任务
});

// Multi-threaded lock-free download worker
async fn download_worker_multi(
    mut download_rx: mpsc::Receiver<DownloadTask>,
    extract_senders: Vec<mpsc::Sender<StreamingExtractTask>>,
    deduplicator: Arc<DownloadDeduplicator>,
    worker_id: usize,
) {
    use crate::util::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};
    use once_cell::sync::Lazy;
    use reqwest::Client;
    use reqwest::StatusCode;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio_retry::RetryIf;

    static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);
    static EXTRACT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    log_verbose(&format!("Download worker {worker_id} started"));

    while let Some(task) = download_rx.recv().await {
        log_verbose(&format!(
            "Worker {} downloading {}",
            worker_id, task.key.name
        ));

        // Pure download, get byte data
        let download_result = RetryIf::spawn(
            create_retry_strategy(),
            || async {
                let response = DOWNLOADER_CLIENT
                    .get(&task.key.resolved_url)
                    .send()
                    .await
                    .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

                match response.status() {
                    StatusCode::OK => {
                        let bytes = response.bytes().await.map_err(|e| {
                            RetryableError::Temporary(format!("Failed to read response: {e}"))
                        })?;
                        Ok(bytes.to_vec())
                    }
                    StatusCode::NOT_FOUND => {
                        log_verbose(&format!("URL not found {}", task.key.resolved_url));
                        Err(RetryableError::Permanent(format!(
                            "URL not found {}",
                            task.key.resolved_url
                        )))
                    }
                    status => {
                        log_verbose(&format!(
                            "Error: {status}, url: {}, retrying",
                            task.key.resolved_url
                        ));
                        Err(RetryableError::Temporary(format!(
                            "HTTP error: {status}, url: {}",
                            task.key.resolved_url
                        )))
                    }
                }
            },
            |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
        )
        .await;

        match download_result {
            Ok(data) => {
                log_progress(&format!("{} downloaded", task.key.name));

                // Get clone target list
                if let Some(clone_targets) = deduplicator.complete_package(&task.key) {
                    // Round-robin distribute to streaming extract worker
                    let extract_idx =
                        EXTRACT_COUNTER.fetch_add(1, Ordering::Relaxed) % extract_senders.len();
                    let extract_task = StreamingExtractTask {
                        downloaded_data: data,
                        cache_path: task.cache_path,
                        package_key: task.key,
                        has_install_script: task.has_install_script,
                        clone_targets,
                    };

                    if let Err(e) = extract_senders[extract_idx].send(extract_task).await {
                        log_verbose(&format!(
                            "Worker {worker_id} failed to send streaming extract task: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                log_verbose(&format!(
                    "Worker {} download failed: url={}, error={}",
                    worker_id, task.key.resolved_url, e
                ));

                // Download failed, remove from deduplicator to avoid permanent blocking
                deduplicator.complete_package(&task.key);
            }
        }
    }
    log_verbose(&format!("Download worker {worker_id} finished"));
}

// Streaming extract worker - directly write files without intermediate storage
async fn streaming_extract_worker(
    mut extract_rx: mpsc::Receiver<StreamingExtractTask>,
    clone_senders: Vec<mpsc::Sender<CloneTask>>,
    worker_id: usize,
) {
    use async_compression::tokio::bufread::GzipDecoder;
    use futures::StreamExt;
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::fs;
    use tokio::io::BufReader;
    use tokio_tar::Archive;

    static CLONE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    log_verbose(&format!("Streaming extract worker {worker_id} started"));

    while let Some(task) = extract_rx.recv().await {
        log_verbose(&format!(
            "Worker {} streaming extract {} directly to disk",
            worker_id, task.package_key.name
        ));

        // Extract and write files in streaming fashion
        let tar_gz = GzipDecoder::new(BufReader::new(&task.downloaded_data[..]));
        let mut archive = Archive::new(tar_gz);
        let mut files_extracted = 0;

        // Read and immediately write each file
        let mut entries = match archive.entries() {
            Ok(entries) => entries,
            Err(e) => {
                log_verbose(&format!(
                    "Worker {} failed to read archive entries for {}: {}",
                    worker_id, task.package_key.name, e
                ));
                continue;
            }
        };

        let mut extraction_success = true;

        while let Some(entry) = entries.next().await {
            let mut file = match entry {
                Ok(file) => file,
                Err(e) => {
                    log_verbose(&format!(
                        "Worker {} failed to read file entry for {}: {}",
                        worker_id, task.package_key.name, e
                    ));
                    extraction_success = false;
                    break;
                }
            };

            let path = match file.path() {
                Ok(path) => path.into_owned(),
                Err(e) => {
                    log_verbose(&format!(
                        "Worker {} failed to get file path for {}: {}",
                        worker_id, task.package_key.name, e
                    ));
                    continue;
                }
            };

            // Skip directories
            if file.header().entry_type().is_dir() {
                continue;
            }

            // Get original permissions
            let original_mode = file.header().mode().unwrap_or(0o644);

            // Construct target file path
            let file_path = task.cache_path.join(&path);

            // Create parent directory
            if let Some(parent) = file_path.parent()
                && let Err(e) = fs::create_dir_all(parent).await
            {
                log_verbose(&format!(
                    "Worker {} failed to create directory {}: {}",
                    worker_id,
                    parent.display(),
                    e
                ));
                extraction_success = false;
                break;
            }

            // Stream file content directly to disk
            let mut temp_file = match tokio::fs::File::create(&file_path).await {
                Ok(f) => f,
                Err(e) => {
                    log_verbose(&format!(
                        "Worker {} failed to create file {}: {}",
                        worker_id,
                        file_path.display(),
                        e
                    ));
                    extraction_success = false;
                    break;
                }
            };

            if let Err(e) = tokio::io::copy(&mut file, &mut temp_file).await {
                log_verbose(&format!(
                    "Worker {} failed to write file content for {} in {}: {}",
                    worker_id,
                    path.display(),
                    task.package_key.name,
                    e
                ));
                extraction_success = false;
                break;
            }

            // Set original permissions
            let permissions = Permissions::from_mode(original_mode);
            if let Err(e) = fs::set_permissions(&file_path, permissions).await {
                log_verbose(&format!(
                    "Worker {} failed to set permissions for {}: {}",
                    worker_id,
                    file_path.display(),
                    e
                ));
            }

            files_extracted += 1;
        }

        if !extraction_success {
            log_verbose(&format!(
                "Worker {} extraction failed for {}",
                worker_id, task.package_key.name
            ));
            continue;
        }

        if files_extracted == 0 {
            log_verbose(&format!(
                "Worker {} no files extracted for {}",
                worker_id, task.package_key.name
            ));
            continue;
        }

        log_verbose(&format!(
            "Worker {} successfully extracted {} files for {} directly to disk",
            worker_id, files_extracted, task.package_key.name
        ));

        // Create completion tracker
        let completion_tracker = PackageCompletionTracker::new(
            task.package_key.clone(),
            task.cache_path.clone(),
            task.clone_targets,
            task.has_install_script,
        );

        // All files written successfully, create _resolved marker
        let resolved_path = completion_tracker.cache_path.join("_resolved");
        if let Err(e) = fs::write(&resolved_path, "").await {
            log_verbose(&format!(
                "Worker {} failed to write _resolved for {}: {}",
                worker_id, completion_tracker.package_key.name, e
            ));
            continue;
        }

        // Write _hasInstallScript marker if needed
        if completion_tracker.has_install_script.is_some() {
            let install_script_path = completion_tracker.cache_path.join("_hasInstallScript");
            if let Err(e) = fs::write(&install_script_path, "").await {
                log_verbose(&format!(
                    "Worker {} failed to write _hasInstallScript for {}: {}",
                    worker_id, completion_tracker.package_key.name, e
                ));
            }
        }

        log_verbose(&format!(
            "Worker {} package {} cache completed via streaming",
            worker_id, completion_tracker.package_key.name
        ));

        // Round-robin distribute clone tasks
        for (clone_idx, clone_target) in completion_tracker.clone_targets.iter().enumerate() {
            let sender_idx =
                (CLONE_COUNTER.fetch_add(1, Ordering::Relaxed) + clone_idx) % clone_senders.len();
            let clone_task = CloneTask {
                cache_path: completion_tracker.cache_path.clone(),
                target_path: clone_target.target_path.clone(),
                package_name: clone_target.package_name.clone(),
            };

            if let Err(e) = clone_senders[sender_idx].send(clone_task).await {
                log_verbose(&format!(
                    "Worker {worker_id} failed to send clone task: {e}"
                ));
            }
        }
    }
    log_verbose(&format!("Streaming extract worker {worker_id} finished"));
}

// Multi-threaded lock-free clone worker
async fn clone_worker_multi(mut clone_rx: mpsc::Receiver<CloneTask>, worker_id: usize) {
    use crate::util::cloner::clone;

    log_verbose(&format!("Clone worker {worker_id} started"));

    while let Some(task) = clone_rx.recv().await {
        log_verbose(&format!("Worker {} clone {}", worker_id, task.package_name));

        match clone(&task.cache_path, &task.target_path, true).await {
            Ok(_) => {
                log_verbose(&format!(
                    "Worker {} {} resolved",
                    worker_id, task.package_name
                ));
                PROGRESS_BAR.inc(1);
                log_progress(&format!("{} resolved", task.package_name));

                // Update package binary
                if let Err(e) = update_package_binary(&task.target_path, &task.package_name).await {
                    log_verbose(&format!(
                        "Worker {} failed to update binary for {}: {}",
                        worker_id, task.package_name, e
                    ));
                }

                // Decrement depth completion counter
                DEPTH_COMPLETION_COUNTER.fetch_sub(1, Ordering::SeqCst);
            }
            Err(e) => {
                log_verbose(&format!(
                    "Worker {} copy failed {} to {}: {}",
                    worker_id,
                    task.cache_path.display(),
                    task.target_path.display(),
                    e
                ));
            }
        }
    }
    log_verbose(&format!("Clone worker {worker_id} finished"));
}

// 统一的下载和解压函数
pub async fn download_and_extract_package(
    name: &str,
    version: &str,
    tarball_url: &str,
    cache_path: &Path,
    has_install_script: bool,
    clone_targets: Option<Vec<PathBuf>>,
) -> Result<()> {
    use crate::util::retry::{RetryableError, build_dns_cached_client, create_retry_strategy};
    use once_cell::sync::Lazy;
    use reqwest::Client;
    use reqwest::StatusCode;
    use tokio_retry::RetryIf;

    static UNIFIED_DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);

    let _permit = if clone_targets.is_none() {
        // 预下载需要获取信号量许可
        Some(PRE_DOWNLOAD_SEMAPHORE.acquire().await.unwrap())
    } else {
        None
    };

    log_verbose(&format!("Downloading {name}@{version} from {tarball_url}"));

    // 下载
    let download_result = RetryIf::spawn(
        create_retry_strategy(),
        || async {
            let response = UNIFIED_DOWNLOADER_CLIENT
                .get(tarball_url)
                .send()
                .await
                .map_err(|e| RetryableError::Temporary(format!("Network error: {e}")))?;

            match response.status() {
                StatusCode::OK => {
                    let bytes = response.bytes().await.map_err(|e| {
                        RetryableError::Temporary(format!("Failed to read response: {e}"))
                    })?;
                    Ok(bytes.to_vec())
                }
                StatusCode::NOT_FOUND => {
                    log_verbose(&format!("URL not found {tarball_url}"));
                    Err(RetryableError::Permanent(format!(
                        "URL not found {tarball_url}"
                    )))
                }
                status => {
                    log_verbose(&format!("Error: {status}, url: {tarball_url}, retrying"));
                    Err(RetryableError::Temporary(format!(
                        "HTTP error: {status}, url: {tarball_url}"
                    )))
                }
            }
        },
        |e: &RetryableError| matches!(e, RetryableError::Temporary(_)),
    )
    .await?;

    // 解压到缓存
    use async_compression::tokio::bufread::GzipDecoder;
    use futures::StreamExt;
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    use tokio::fs;
    use tokio::io::BufReader;
    use tokio_tar::Archive;

    let tar_gz = GzipDecoder::new(BufReader::new(&download_result[..]));
    let mut archive = Archive::new(tar_gz);
    let mut files_extracted = 0;

    let mut entries = archive.entries()?;

    while let Some(entry) = entries.next().await {
        let mut file = entry?;

        let path = file.path()?.into_owned();

        if file.header().entry_type().is_dir() {
            continue;
        }

        let original_mode = file.header().mode().unwrap_or(0o644);
        let file_path = cache_path.join(&path);

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut temp_file = tokio::fs::File::create(&file_path).await?;
        tokio::io::copy(&mut file, &mut temp_file).await?;

        let permissions = Permissions::from_mode(original_mode);
        fs::set_permissions(&file_path, permissions).await?;

        files_extracted += 1;
    }

    if files_extracted == 0 {
        return Err(anyhow::anyhow!("No files extracted from tarball"));
    }

    // 创建标记文件
    let resolved_path = cache_path.join("_resolved");
    fs::write(&resolved_path, "").await?;

    if has_install_script {
        let install_script_path = cache_path.join("_hasInstallScript");
        fs::write(&install_script_path, "").await?;
    }

    log_verbose(&format!(
        "Downloaded and extracted {}@{}: {} files to {}",
        name,
        version,
        files_extracted,
        cache_path.display()
    ));

    // 如果有clone目标，则进行clone
    if let Some(targets) = clone_targets {
        for target in targets {
            if let Err(e) = crate::util::cloner::clone(cache_path, &target, false).await {
                log_verbose(&format!(
                    "Failed to clone {}@{} from {} to {}: {}",
                    name,
                    version,
                    cache_path.display(),
                    target.display(),
                    e
                ));
            } else {
                log_verbose(&format!(
                    "Cloned {}@{} from cache to {}",
                    name,
                    version,
                    target.display()
                ));
            }
        }
    }

    Ok(())
}

pub async fn install_packages_optimized(
    groups: &HashMap<usize, Vec<(std::string::String, Package)>>,
    cache_dir: &Path,
    cwd: &Path,
) -> Result<()> {
    // clean unused deps
    clean_deps(groups, cwd).await?;

    let mut depths: Vec<_> = groups.keys().cloned().collect();
    depths.sort_unstable();

    let thread_count = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    // Create multiple channels for streaming parallelism (no separate write workers needed)
    let download_worker_count = thread_count * 2;
    let extract_worker_count = thread_count;
    let clone_worker_count = thread_count;

    log_verbose(&format!(
        "Streaming workers - download: {download_worker_count}, extract: {extract_worker_count}, clone: {clone_worker_count}"
    ));

    // Create round-robin channels for streaming pipeline
    let mut download_channels = Vec::new();
    let mut extract_channels = Vec::new();
    let mut clone_channels = Vec::new();

    for _ in 0..download_worker_count {
        let (tx, rx) = mpsc::channel::<DownloadTask>(50);
        download_channels.push((tx, rx));
    }

    for _ in 0..extract_worker_count {
        let (tx, rx) = mpsc::channel::<StreamingExtractTask>(30); // Smaller buffer since no intermediate storage
        extract_channels.push((tx, rx));
    }

    for _ in 0..clone_worker_count {
        let (tx, rx) = mpsc::channel::<CloneTask>(50);
        clone_channels.push((tx, rx));
    }

    // Create deduplicator
    let deduplicator = Arc::new(DownloadDeduplicator::new());

    // Extract senders for streaming pipeline
    let download_senders: Vec<_> = download_channels.iter().map(|(tx, _)| tx.clone()).collect();
    let extract_senders: Vec<_> = extract_channels.iter().map(|(tx, _)| tx.clone()).collect();
    let clone_senders: Vec<_> = clone_channels.iter().map(|(tx, _)| tx.clone()).collect();

    let mut workers = Vec::new();

    // Spawn download workers
    for (i, (_, rx)) in download_channels.into_iter().enumerate() {
        let extract_senders_clone = extract_senders.clone();
        let deduplicator_clone = Arc::clone(&deduplicator);

        let worker = tokio::spawn(async move {
            download_worker_multi(rx, extract_senders_clone, deduplicator_clone, i).await;
        });
        workers.push(worker);
    }

    // Spawn streaming extract workers
    for (i, (_, rx)) in extract_channels.into_iter().enumerate() {
        let clone_senders_clone = clone_senders.clone();

        let worker = tokio::spawn(async move {
            streaming_extract_worker(rx, clone_senders_clone, i).await;
        });
        workers.push(worker);
    }

    // Spawn clone workers
    for (i, (_, rx)) in clone_channels.into_iter().enumerate() {
        let worker = tokio::spawn(async move {
            clone_worker_multi(rx, i).await;
        });
        workers.push(worker);
    }

    // Process packages by depth using deduplicator - maintain batch processing for layer ordering
    for depth in depths.iter() {
        log_verbose(&format!("Processing depth level {depth}"));

        if let Some(packages) = groups.get(depth) {
            let mut batch_tasks = Vec::new();

            // Prepare all tasks for this depth level
            for (path, package) in packages.iter() {
                if let Some(resolved) = &package.resolved {
                    if package.link.is_some() {
                        let link_name = extract_package_name(path);
                        if link_name.is_empty() {
                            PROGRESS_BAR.inc(1);
                            log_verbose(&format!("Link skipped due to empty package name: {path}"));
                            continue;
                        }
                        log_verbose(&format!("Attempting to link from {resolved} to {path}"));
                        if let Err(e) = link(Path::new(&resolved), Path::new(&path)) {
                            log_verbose(&format!(
                                "Link failed: source={resolved}, target={path}, error={e}"
                            ));
                            return Err(anyhow::anyhow!("Link failed: {e}"));
                        }
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    // skip when cpu or os is not compatible
                    if package.cpu.is_some() && !is_cpu_compatible(package.cpu.as_ref().unwrap()) {
                        PROGRESS_BAR.inc(1);
                        log_verbose(&format!("cpu skipped: {}", &path));
                        continue;
                    }

                    if package.os.is_some() && !is_os_compatible(package.os.as_ref().unwrap()) {
                        PROGRESS_BAR.inc(1);
                        log_verbose(&format!("os skipped: {}", &path));
                        continue;
                    }

                    let name = package
                        .name
                        .clone()
                        .unwrap_or_else(|| extract_package_name(path));
                    let version = package.version.as_ref().unwrap();
                    let cache_path = cache_dir.join(format!("{name}/{version}"));
                    let cache_flag_path = cache_dir.join(format!("{name}/{version}/_resolved"));
                    let target_path = cwd.join(path);

                    // Create package key and clone target
                    let package_key = PackageKey {
                        name: name.clone(),
                        version: version.clone(),
                        resolved_url: resolved.clone(),
                    };

                    let clone_target = CloneTarget {
                        target_path,
                        package_name: name.clone(),
                    };

                    // Prepare task for batch execution
                    batch_tasks.push((
                        package_key,
                        clone_target,
                        cache_path,
                        cache_flag_path,
                        package.has_install_script,
                    ));
                } else {
                    PROGRESS_BAR.inc(1);
                    log_verbose(&format!("{path} no resolved info skipped"));
                }
            }

            // Execute all tasks in this batch level concurrently
            if batch_tasks.is_empty() {
                log_verbose(&format!("Depth level {depth} has no tasks, skipping"));
                continue;
            }

            log_verbose(&format!(
                "Executing {} tasks for depth level {}",
                batch_tasks.len(),
                depth
            ));

            // Reset and set depth completion counter for this batch
            DEPTH_COMPLETION_COUNTER.store(batch_tasks.len(), Ordering::SeqCst);
            let mut task_counter = 0;

            for (package_key, clone_target, cache_path, cache_flag_path, has_install_script) in
                batch_tasks
            {
                // Check status and register with deduplicator
                match deduplicator.check_and_register(
                    package_key.clone(),
                    clone_target.clone(),
                    &cache_flag_path,
                ) {
                    PackageStatus::AlreadyCached => {
                        // Direct clone from cache using round-robin
                        let clone_task = CloneTask {
                            cache_path,
                            target_path: clone_target.target_path,
                            package_name: clone_target.package_name,
                        };

                        let clone_idx = task_counter % clone_senders.len();
                        if let Err(e) = clone_senders[clone_idx].send(clone_task).await {
                            log_verbose(&format!(
                                "Failed to send direct clone task for {}: {}",
                                package_key.name, e
                            ));
                        }
                        task_counter += 1;
                    }
                    PackageStatus::NeedDownload => {
                        // Create download task using round-robin
                        let download_task = DownloadTask {
                            key: package_key.clone(),
                            cache_path,
                            has_install_script,
                        };

                        let download_idx = task_counter % download_senders.len();
                        if let Err(e) = download_senders[download_idx].send(download_task).await {
                            log_verbose(&format!(
                                "Failed to send download task for {}: {}",
                                package_key.name, e
                            ));
                        }
                        task_counter += 1;
                    }
                    PackageStatus::InProgress => {
                        // Already being downloaded by another task, just wait
                        log_verbose(&format!(
                            "Package {} already in progress, added to clone targets",
                            package_key.name
                        ));
                    }
                }
            }

            // Wait for this depth level to complete before moving to next depth level
            log_verbose(&format!(
                "Waiting for depth level {depth} ({task_counter} tasks) to complete"
            ));

            // Wait for all tasks in this depth to complete
            while DEPTH_COMPLETION_COUNTER.load(Ordering::SeqCst) > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }

            log_verbose(&format!(
                "Depth level {depth} completed, proceeding to next level"
            ));
        }
    }

    // Close all channels to signal workers to finish
    drop(download_senders);
    drop(extract_senders);
    drop(clone_senders);

    // Wait for all workers to complete
    for worker in workers {
        if let Err(e) = worker.await {
            log_verbose(&format!("Worker error: {e}"));
        }
    }

    Ok(())
}

pub struct InstallService;

impl InstallService {
    pub async fn update_packages(
        action: PackageAction,
        specs: &[&str],
        workspace: Option<String>,
        ignore_scripts: bool,
        save_type: SaveType,
    ) -> Result<()> {
        log_verbose(&format!(
            "update packages: {:?} {:?} {:?} {:?}",
            action, specs, &workspace, ignore_scripts
        ));

        if specs.is_empty() {
            return Err(anyhow::anyhow!("No package specifications provided"));
        }

        let cwd = std::env::current_dir().context("Failed to get current directory")?;

        // Update working directory to project root (if in workspace)
        let root_path = crate::helper::workspace::update_cwd_to_root(&cwd).await?;

        // Update package.json and package-lock.json for all packages in batch
        update_package_json(&root_path, &action, specs, &workspace, &save_type)
            .await
            .context("Failed to update package.json")?;

        // Rebuild Deps
        crate::cmd::deps::build_deps(&root_path)
            .await
            .context("Failed to build package-lock.json")?;

        Self::install(ignore_scripts, &root_path)
            .await
            .context("Failed to install packages")?;

        Ok(())
    }

    pub async fn install(ignore_scripts: bool, root_path: &Path) -> Result<()> {
        // Package lock prerequisite check
        ensure_package_lock(root_path).await?;

        // load package-lock.json
        let package_lock: PackageLock = serde_json::from_reader(
            std::fs::File::open(root_path.join("package-lock.json"))
                .context("Failed to open package-lock.json")?,
        )
        .map_err(|e| anyhow::anyhow!("Failed to parse package-lock.json: {}", e))?;

        let cache_dir = get_cache_dir();

        let groups = group_by_depth(&package_lock.packages);

        let mut depths: Vec<_> = groups.keys().cloned().collect();
        depths.sort_unstable();
        if !package_lock.packages.is_empty() {
            start_progress_bar();
            PROGRESS_BAR.set_length(package_lock.packages.len() as u64);
        }

        install_packages_optimized(&groups, &cache_dir, root_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to install packages: {}", e))?;

        finish_progress_bar("node_modules cloned");

        if !ignore_scripts {
            log_info(
                "Starting to execute dependency hook scripts, you can add --ignore-scripts to skip",
            );
            rebuild(root_path).await?;
            log_info("💫 All dependencies installed successfully");
            Ok(())
        } else {
            log_info(
                "💫 All dependencies installed successfully (you can run 'utoo rebuild' to trigger dependency hooks)",
            );
            Ok(())
        }
    }

    pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
        // Prepare global package directory and package.json
        let package_path = prepare_global_package_json(npm_spec, prefix)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to prepare global package.json: {}", e))?;

        log_verbose(&format!("Installing global package: {npm_spec}"));

        // Install dependencies
        Self::install(false, &package_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to install global package dependencies: {}", e))?;

        // Create package info from path
        let package_info = PackageInfo::from_path(&package_path)
            .context("Failed to create package info from path")?;

        // Get global bin directory using the common helper
        let target_bin_dir =
            get_global_bin_dir(prefix).context("Failed to get global bin directory")?;

        // Link binary files to global
        log_verbose(&format!(
            "Linking binary files to global... {}",
            target_bin_dir.display()
        ));
        package_info
            .link_to_global(&target_bin_dir)
            .await
            .context("Failed to link binary files to global")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_clean_symlink() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let target_dir = temp_dir.path().join("utoo-cli");
        let symlink_path = temp_dir.path().join("symlink");

        // Create target directory
        fs::create_dir(&target_dir).await?;

        // Create symlink
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlink_path)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target_dir, &symlink_path)?;

        // Test cleaning
        clean_symlink(&symlink_path).await?;

        // Verify symlink is removed
        assert!(!symlink_path.exists());
        assert!(target_dir.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_clean_scoped_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let scope_dir = temp_dir.path().join("@utoo");
        fs::create_dir(&scope_dir).await?;

        // Create a symlink in the scope directory
        let target_dir = temp_dir.path().join("utoo-cli");
        let symlink_path = scope_dir.join("cli");
        fs::create_dir(&target_dir).await?;

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlink_path)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target_dir, &symlink_path)?;

        // Test cleaning
        clean_scoped_package(&scope_dir).await?;

        // Verify symlink is removed
        assert!(!symlink_path.exists());
        assert!(target_dir.exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_clean_legacy_npminstall_package() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let legacy_dir = temp_dir.path().join("_utoo-cli@1.0.0@2.0.0");
        fs::create_dir(&legacy_dir).await?;

        // Test cleaning
        clean_legacy_npminstall_package(&legacy_dir, "_utoo-cli@1.0.0@2.0.0").await?;

        // Verify directory is removed
        assert!(!legacy_dir.exists());

        Ok(())
    }

    #[test]
    fn test_extract_package_name_from_path() {
        // Test extracting package name from a standard path
        assert_eq!(extract_package_name("node_modules/lodash"), "lodash");

        // Test extracting package name from a nested path
        assert_eq!(
            extract_package_name("node_modules/parent/node_modules/child"),
            "child"
        );

        // Test extracting package name from a scoped package path
        assert_eq!(
            extract_package_name("node_modules/@scope/package"),
            "@scope/package"
        );
    }
}
