use anyhow::Context;
use anyhow::Result;
use glob::glob;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::thread;
use tokio::sync::mpsc;
use std::path::PathBuf;
use dashmap::DashMap;

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

// 包的唯一标识
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct PackageKey {
    name: String,
    version: String,
    resolved_url: String,
}

// Clone目标信息
#[derive(Debug, Clone)]
struct CloneTarget {
    target_path: PathBuf,
    package_name: String,
}

// 下载任务
#[derive(Debug)]
struct DownloadTask {
    key: PackageKey,
    cache_path: PathBuf,
    has_install_script: Option<bool>,
}

// 解压任务
#[derive(Debug)]
struct ExtractTask {
    downloaded_data: Vec<u8>,
    cache_path: PathBuf,
    package_key: PackageKey,
    has_install_script: Option<bool>,
    clone_targets: Vec<CloneTarget>,
}

// 文件写入任务
#[derive(Debug)]
struct FileWriteTask {
    file_path: PathBuf,
    file_data: Vec<u8>,
    file_mode: u32, // 存储原始文件权限
    package_tracker: Arc<PackageCacheTracker>,
}

// Clone任务
#[derive(Debug)]
struct CloneTask {
    cache_path: PathBuf,
    target_path: PathBuf,
    package_name: String,
    has_install_script: Option<bool>,
}

// 包缓存跟踪器
#[derive(Debug)]
struct PackageCacheTracker {
    package_key: PackageKey,
    cache_path: PathBuf,
    clone_targets: Vec<CloneTarget>,
    total_files: usize,
    files_written: AtomicUsize,
    has_install_script: Option<bool>,
}

// 去重管理器
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

    /// 检查包状态并注册clone目标
    fn check_and_register(&self,
        key: PackageKey,
        clone_target: CloneTarget,
        cache_flag_path: &Path
    ) -> PackageStatus {
        // 检查缓存是否存在
        if cache_flag_path.exists() {
            return PackageStatus::AlreadyCached;
        }

        // 尝试注册到正在进行的下载任务
        match self.in_progress.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // 已有下载任务，添加clone目标
                entry.get_mut().push(clone_target);
                PackageStatus::InProgress
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // 新的下载任务，创建clone目标列表
                entry.insert(vec![clone_target]);
                PackageStatus::NeedDownload
            }
        }
    }

    /// 获取并移除完成的包的clone目标列表
    fn complete_package(&self, key: &PackageKey) -> Option<Vec<CloneTarget>> {
        self.in_progress.remove(key).map(|(_, targets)| targets)
    }
}

impl PackageCacheTracker {
    fn new(
        package_key: PackageKey,
        cache_path: PathBuf,
        clone_targets: Vec<CloneTarget>,
        total_files: usize,
        has_install_script: Option<bool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            package_key,
            cache_path,
            clone_targets,
            total_files,
            files_written: AtomicUsize::new(0),
            has_install_script,
        })
    }

    /// 标记一个文件写入完成，返回是否所有文件都已完成
    fn mark_file_written(&self) -> bool {
        let written = self.files_written.fetch_add(1, Ordering::SeqCst) + 1;
        written >= self.total_files
    }
}


// 多线程无锁 download worker
async fn download_worker_multi(
    mut download_rx: mpsc::Receiver<DownloadTask>,
    extract_senders: Vec<mpsc::Sender<ExtractTask>>,
    deduplicator: Arc<DownloadDeduplicator>,
    worker_id: usize,
) {
    use crate::util::retry::{RetryableError, create_retry_strategy, build_dns_cached_client};
    use once_cell::sync::Lazy;
    use reqwest::Client;
    use tokio_retry::RetryIf;
    use reqwest::StatusCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DOWNLOADER_CLIENT: Lazy<Client> = Lazy::new(build_dns_cached_client);
    static EXTRACT_COUNTER: AtomicUsize = AtomicUsize::new(0);

    log_verbose(&format!("Download worker {} started", worker_id));

    while let Some(task) = download_rx.recv().await {
        log_verbose(&format!("Worker {} downloading {}", worker_id, task.key.name));

        // 纯下载，获取字节数据
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
                        Err(RetryableError::Permanent(format!("URL not found {}", task.key.resolved_url)))
                    }
                    status => {
                        log_verbose(&format!("Error: {status}, url: {}, retrying", task.key.resolved_url));
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

                // 获取clone目标列表
                if let Some(clone_targets) = deduplicator.complete_package(&task.key) {
                    // Round-robin 分发到解压 worker
                    let extract_idx = EXTRACT_COUNTER.fetch_add(1, Ordering::Relaxed) % extract_senders.len();
                    let extract_task = ExtractTask {
                        downloaded_data: data,
                        cache_path: task.cache_path,
                        package_key: task.key,
                        has_install_script: task.has_install_script,
                        clone_targets,
                    };

                    if let Err(e) = extract_senders[extract_idx].send(extract_task).await {
                        log_verbose(&format!("Worker {} failed to send extract task: {}", worker_id, e));
                    }
                }
            }
            Err(e) => {
                log_verbose(&format!(
                    "Worker {} download failed: url={}, error={}",
                    worker_id,
                    task.key.resolved_url,
                    e
                ));

                // 下载失败，从去重器中移除，避免永久阻塞
                deduplicator.complete_package(&task.key);
            }
        }
    }
    log_verbose(&format!("Download worker {} finished", worker_id));
}

// 多线程无锁 extract worker
async fn extract_worker_multi(
    mut extract_rx: mpsc::Receiver<ExtractTask>,
    write_senders: Vec<mpsc::Sender<FileWriteTask>>,
    worker_id: usize,
) {
    use async_compression::tokio::bufread::GzipDecoder;
    use tokio::io::BufReader;
    use tokio_tar::Archive;
    use std::collections::HashMap;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static WRITE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    log_verbose(&format!("Extract worker {} started", worker_id));

    while let Some(task) = extract_rx.recv().await {
        log_verbose(&format!("Worker {} extracting {} to memory", worker_id, task.package_key.name));

        // 解压到内存
        let tar_gz = GzipDecoder::new(BufReader::new(&task.downloaded_data[..]));
        let mut archive = Archive::new(tar_gz);
        let mut files: HashMap<PathBuf, (Vec<u8>, u32)> = HashMap::new(); // (文件内容, 权限)

        // 读取所有文件到内存
        let mut entries = match archive.entries() {
            Ok(entries) => entries,
            Err(e) => {
                log_verbose(&format!("Worker {} failed to read archive entries for {}: {}", worker_id, task.package_key.name, e));
                continue;
            }
        };

        while let Some(entry) = entries.next().await {
            let mut file = match entry {
                Ok(file) => file,
                Err(e) => {
                    log_verbose(&format!("Worker {} failed to read file entry for {}: {}", worker_id, task.package_key.name, e));
                    continue;
                }
            };

            let path = match file.path() {
                Ok(path) => path.into_owned(),
                Err(e) => {
                    log_verbose(&format!("Worker {} failed to get file path for {}: {}", worker_id, task.package_key.name, e));
                    continue;
                }
            };

            // 跳过目录
            if file.header().entry_type().is_dir() {
                continue;
            }

            // 获取原始权限
            let original_mode = file.header().mode().unwrap_or(0o644);

            // 读取文件内容到内存
            let mut content = Vec::new();
            if let Err(e) = tokio::io::copy(&mut file, &mut content).await {
                log_verbose(&format!("Worker {} failed to read file content for {} in {}: {}", worker_id, path.display(), task.package_key.name, e));
                continue;
            }

            files.insert(path, (content, original_mode));
        }

        if files.is_empty() {
            log_verbose(&format!("Worker {} no files extracted for {}", worker_id, task.package_key.name));
            continue;
        }

        log_verbose(&format!("Worker {} extracted {} files for {} to memory", worker_id, files.len(), task.package_key.name));

        // 创建包跟踪器
        let tracker = PackageCacheTracker::new(
            task.package_key,
            task.cache_path.clone(),
            task.clone_targets,
            files.len(),
            task.has_install_script,
        );

        // Round-robin 分发文件写入任务
        for (file_idx, (relative_path, (file_data, file_mode))) in files.into_iter().enumerate() {
            let file_path = task.cache_path.join(&relative_path);
            let write_idx = (WRITE_COUNTER.fetch_add(1, Ordering::Relaxed) + file_idx) % write_senders.len();

            let write_task = FileWriteTask {
                file_path,
                file_data,
                file_mode,
                package_tracker: Arc::clone(&tracker),
            };

            if let Err(e) = write_senders[write_idx].send(write_task).await {
                log_verbose(&format!("Worker {} failed to send write task: {}", worker_id, e));
                break;
            }
        }
    }
    log_verbose(&format!("Extract worker {} finished", worker_id));
}

// 多线程无锁 write worker
async fn write_worker_multi(
    mut write_rx: mpsc::Receiver<FileWriteTask>,
    clone_senders: Vec<mpsc::Sender<CloneTask>>,
    worker_id: usize,
) {
    use tokio::fs;
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CLONE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    log_verbose(&format!("Write worker {} started", worker_id));

    while let Some(task) = write_rx.recv().await {
        // 创建父目录
        if let Some(parent) = task.file_path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                log_verbose(&format!("Worker {} failed to create directory {}: {}", worker_id, parent.display(), e));
                continue;
            }
        }

        // 写入文件
        if let Err(e) = fs::write(&task.file_path, &task.file_data).await {
            log_verbose(&format!("Worker {} failed to write file {}: {}", worker_id, task.file_path.display(), e));
            continue;
        }

        // 使用tar文件中的原始权限
        let permissions = Permissions::from_mode(task.file_mode);
        if let Err(e) = fs::set_permissions(&task.file_path, permissions).await {
            log_verbose(&format!("Worker {} failed to set permissions for {}: {}", worker_id, task.file_path.display(), e));
        }

        // 标记文件写入完成
        let all_files_written = task.package_tracker.mark_file_written();

        if all_files_written {
            // 所有文件都已写入，创建 _resolved 标记
            let resolved_path = task.package_tracker.cache_path.join("_resolved");
            if let Err(e) = fs::write(&resolved_path, "").await {
                log_verbose(&format!("Worker {} failed to write _resolved for {}: {}", worker_id, task.package_tracker.package_key.name, e));
                continue;
            }

            // 写入 _hasInstallScript 标记
            if task.package_tracker.has_install_script.is_some() {
                let install_script_path = task.package_tracker.cache_path.join("_hasInstallScript");
                if let Err(e) = fs::write(&install_script_path, "").await {
                    log_verbose(&format!("Worker {} failed to write _hasInstallScript for {}: {}", worker_id, task.package_tracker.package_key.name, e));
                }
            }

            log_verbose(&format!("Worker {} package {} cache completed", worker_id, task.package_tracker.package_key.name));

            // Round-robin 分发 clone 任务
            for (clone_idx, clone_target) in task.package_tracker.clone_targets.iter().enumerate() {
                let sender_idx = (CLONE_COUNTER.fetch_add(1, Ordering::Relaxed) + clone_idx) % clone_senders.len();
                let clone_task = CloneTask {
                    cache_path: task.package_tracker.cache_path.clone(),
                    target_path: clone_target.target_path.clone(),
                    package_name: clone_target.package_name.clone(),
                    has_install_script: task.package_tracker.has_install_script,
                };

                if let Err(e) = clone_senders[sender_idx].send(clone_task).await {
                    log_verbose(&format!("Worker {} failed to send clone task: {}", worker_id, e));
                }
            }
        }
    }
    log_verbose(&format!("Write worker {} finished", worker_id));
}

// 多线程无锁 clone worker
async fn clone_worker_multi(
    mut clone_rx: mpsc::Receiver<CloneTask>,
    worker_id: usize,
) {
    use crate::util::cloner::clone;

    log_verbose(&format!("Clone worker {} started", worker_id));

    while let Some(task) = clone_rx.recv().await {
        log_verbose(&format!("Worker {} clone {}", worker_id, task.package_name));

        match clone(&task.cache_path, &task.target_path, true).await {
            Ok(_) => {
                log_verbose(&format!("Worker {} {} resolved", worker_id, task.package_name));
                PROGRESS_BAR.inc(1);
                log_progress(&format!("{} resolved", task.package_name));

                // 更新package binary
                if let Err(e) = update_package_binary(&task.target_path, &task.package_name).await {
                    log_verbose(&format!("Worker {} failed to update binary for {}: {}", worker_id, task.package_name, e));
                }
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
    log_verbose(&format!("Clone worker {} finished", worker_id));
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

    // Create multiple channels for true lock-free parallelism
    let download_worker_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let extract_worker_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(4); // CPU密集，少一些
    let write_worker_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let clone_worker_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);

    log_verbose(&format!("True lock-free workers - download: {}, extract: {}, write: {}, clone: {}",
        download_worker_count, extract_worker_count, write_worker_count, clone_worker_count));

    // Create round-robin channels for each stage
    let mut download_channels = Vec::new();
    let mut extract_channels = Vec::new();
    let mut write_channels = Vec::new();
    let mut clone_channels = Vec::new();

    for _ in 0..download_worker_count {
        let (tx, rx) = mpsc::channel::<DownloadTask>(50);
        download_channels.push((tx, rx));
    }

    for _ in 0..extract_worker_count {
        let (tx, rx) = mpsc::channel::<ExtractTask>(50);
        extract_channels.push((tx, rx));
    }

    for _ in 0..write_worker_count {
        let (tx, rx) = mpsc::channel::<FileWriteTask>(200);
        write_channels.push((tx, rx));
    }

    for _ in 0..clone_worker_count {
        let (tx, rx) = mpsc::channel::<CloneTask>(50);
        clone_channels.push((tx, rx));
    }

    // Create deduplicator
    let deduplicator = Arc::new(DownloadDeduplicator::new());

    // Extract senders and spawn workers - completely lock-free
    let download_senders: Vec<_> = download_channels.iter().map(|(tx, _)| tx.clone()).collect();
    let extract_senders: Vec<_> = extract_channels.iter().map(|(tx, _)| tx.clone()).collect();
    let write_senders: Vec<_> = write_channels.iter().map(|(tx, _)| tx.clone()).collect();
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

    // Spawn extract workers
    for (i, (_, rx)) in extract_channels.into_iter().enumerate() {
        let write_senders_clone = write_senders.clone();

        let worker = tokio::spawn(async move {
            extract_worker_multi(rx, write_senders_clone, i).await;
        });
        workers.push(worker);
    }

    // Spawn write workers
    for (i, (_, rx)) in write_channels.into_iter().enumerate() {
        let clone_senders_clone = clone_senders.clone();

        let worker = tokio::spawn(async move {
            write_worker_multi(rx, clone_senders_clone, i).await;
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

    // Process packages by depth using deduplicator
    let mut task_counter = 0;
    for depth in depths.iter() {
        if let Some(packages) = groups.get(depth) {
            for (path, package) in packages.iter() {
                if let Some(resolved) = &package.resolved {
                    if package.link.is_some() {
                        let link_name = extract_package_name(&path);
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

                    let name = package.name.as_ref().map(|s| s.clone()).unwrap_or_else(|| extract_package_name(&path));
                    let version = package.version.as_ref().unwrap();
                    let cache_path = cache_dir.join(format!("{name}/{version}"));
                    let cache_flag_path = cache_dir.join(format!("{name}/{version}/_resolved"));
                    let target_path = cwd.join(&path);

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

                    // Check status and register with deduplicator
                    match deduplicator.check_and_register(package_key.clone(), clone_target.clone(), &cache_flag_path) {
                        PackageStatus::AlreadyCached => {
                            // Direct clone from cache using round-robin
                            let clone_task = CloneTask {
                                cache_path,
                                target_path: clone_target.target_path,
                                package_name: clone_target.package_name,
                                has_install_script: package.has_install_script,
                            };

                            let clone_idx = task_counter % clone_senders.len();
                            if let Err(e) = clone_senders[clone_idx].send(clone_task).await {
                                log_verbose(&format!("Failed to send direct clone task for {}: {}", name, e));
                            }
                            task_counter += 1;
                        }
                        PackageStatus::NeedDownload => {
                            // Create download task using round-robin
                            let download_task = DownloadTask {
                                key: package_key,
                                cache_path,
                                has_install_script: package.has_install_script,
                            };

                            let download_idx = task_counter % download_senders.len();
                            if let Err(e) = download_senders[download_idx].send(download_task).await {
                                log_verbose(&format!("Failed to send download task for {}: {}", name, e));
                            }
                            task_counter += 1;
                        }
                        PackageStatus::InProgress => {
                            // Already being downloaded by another task, just wait
                            log_verbose(&format!("Package {} already in progress, added to clone targets", name));
                        }
                    }
                } else {
                    PROGRESS_BAR.inc(1);
                    log_verbose(&format!("{path} no resolved info skipped"));
                }
            }
        }
    }

    // Close all channels to signal workers to finish
    drop(download_senders);
    drop(extract_senders);
    drop(write_senders);
    drop(clone_senders);

    // Wait for all workers to complete
    for worker in workers {
        if let Err(e) = worker.await {
            log_verbose(&format!("Worker error: {}", e));
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
