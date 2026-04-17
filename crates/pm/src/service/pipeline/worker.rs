use std::path::PathBuf;

use super::receiver::PipelineChannels;
use crate::util::cloner::{clone_package_once, wait_clone_if_pending};
use crate::util::downloader::{download_to_cache, is_git_url};

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

/// Start download and clone pipeline workers, returning handles to await completion.
pub fn start_workers(channels: PipelineChannels, cwd: PathBuf) -> PipelineHandles {
    let download_handle = tokio::spawn(async move {
        let mut rx = channels.download_rx;
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
            tokio::spawn(async move {
                download_to_cache(&name, &version, &tarball_url).await;
            });
        }
    });

    let clone_handle = tokio::spawn(async move {
        let mut rx = channels.clone_rx;
        while let Some(msg) = rx.recv().await {
            let Some(tarball_url) = msg.info.tarball_url else {
                continue;
            };
            let name = msg.info.name;
            let version = msg.info.version;
            let target = cwd.join(&msg.path);
            let parent_path = msg.parent_path.map(|p| cwd.join(&p));
            let cwd_for_task = cwd.clone();
            tokio::spawn(async move {
                if let Some(ref parent) = parent_path {
                    wait_clone_if_pending(&parent.to_string_lossy()).await;
                }
                if let Err(e) =
                    clone_package_once(&name, &version, &tarball_url, &target, &cwd_for_task).await
                {
                    tracing::debug!("Pipeline pre-clone failed for {name}@{version}: {e:#}");
                }
            });
        }
    });

    PipelineHandles {
        download_handle,
        clone_handle,
    }
}
