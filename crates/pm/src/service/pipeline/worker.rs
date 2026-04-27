use std::path::PathBuf;

use super::receiver::PipelineChannels;
use crate::util::cloner::{clone_package_once, wait_clone_if_pending};
use crate::util::downloader::{download_to_cache, is_git_url};
use crate::util::user_config::get_manifests_concurrency_limit_sync;
use tokio::task::JoinSet;

/// Pipeline worker handles for awaiting completion.
pub struct PipelineHandles {
    download_handle: tokio::task::JoinHandle<()>,
    clone_handle: tokio::task::JoinHandle<()>,
}

impl PipelineHandles {
    /// Wait for all pipeline workers to complete.
    pub async fn await_completion(self) {
        let _ = self.download_handle.await;
        let _ = self.clone_handle.await;
    }
}

async fn join_next(join_set: &mut JoinSet<()>, worker_name: &str) {
    if let Some(result) = join_set.join_next().await
        && let Err(e) = result
    {
        tracing::debug!("{worker_name} task failed: {e}");
    }
}

async fn drain_tasks(join_set: &mut JoinSet<()>, worker_name: &str) {
    while !join_set.is_empty() {
        join_next(join_set, worker_name).await;
    }
}

/// Start download and clone pipeline workers, returning handles to await completion.
pub fn start_workers(channels: PipelineChannels, cwd: PathBuf) -> PipelineHandles {
    let download_handle = tokio::spawn(async move {
        let mut rx = channels.download_rx;
        let mut tasks = JoinSet::new();
        let max_in_flight = get_manifests_concurrency_limit_sync().max(1);
        while let Some(info) = rx.recv().await {
            let Some(tarball_url) = info.tarball_url else {
                continue;
            };
            // Git packages are cloned & cached during BFS resolution (inside ruborist).
            // Skip the download pipeline — the clone worker will pick up the
            // pre-resolved cache path via resolve_cache_path.
            if is_git_url(&tarball_url) {
                tracing::debug!(
                    "Skipping download for git package: {}@{}",
                    info.name,
                    info.version
                );
                continue;
            }
            let name = info.name;
            let version = info.version;
            tasks.spawn(async move {
                download_to_cache(&name, &version, &tarball_url).await;
            });
            if tasks.len() >= max_in_flight {
                join_next(&mut tasks, "pipeline download").await;
            }
        }
        drain_tasks(&mut tasks, "pipeline download").await;
    });

    let clone_handle = tokio::spawn(async move {
        let mut rx = channels.clone_rx;
        let mut tasks = JoinSet::new();
        let max_in_flight = get_manifests_concurrency_limit_sync().max(1);
        while let Some(msg) = rx.recv().await {
            let Some(tarball_url) = msg.info.tarball_url else {
                continue;
            };
            let name = msg.info.name;
            let version = msg.info.version;
            let target = cwd.join(&msg.path);
            let parent_path = msg.parent_path.map(|p| cwd.join(&p));
            tasks.spawn(async move {
                if let Some(ref parent) = parent_path {
                    wait_clone_if_pending(&parent.to_string_lossy()).await;
                }
                if let Err(e) = clone_package_once(&name, &version, &tarball_url, &target).await {
                    tracing::debug!("Pipeline pre-clone failed for {name}@{version}: {e:#}");
                }
            });
            if tasks.len() >= max_in_flight {
                join_next(&mut tasks, "pipeline clone").await;
            }
        }
        drain_tasks(&mut tasks, "pipeline clone").await;
    });

    PipelineHandles {
        download_handle,
        clone_handle,
    }
}
