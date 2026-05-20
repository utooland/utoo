use std::path::PathBuf;

use super::receiver::PipelineChannels;
use crate::service::install_scheduler::{InstallCloneRequest, InstallScheduler};
use crate::util::downloader::is_registry_tarball_url;

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
pub fn start_workers(
    channels: PipelineChannels,
    cwd: PathBuf,
    scheduler: InstallScheduler,
) -> PipelineHandles {
    let download_scheduler = scheduler.clone();
    let download_handle = tokio::spawn(async move {
        let mut rx = channels.download_rx;
        while let Some(info) = rx.recv().await {
            let Some(tarball_url) = info.tarball_url else {
                continue;
            };
            if !is_registry_tarball_url(&tarball_url) {
                continue;
            }
            if let Err(e) =
                download_scheduler.prefetch_download(info.name, info.version, tarball_url)
            {
                tracing::debug!("Pipeline download prefetch failed: {e:#}");
            }
        }
    });

    let clone_scheduler = scheduler;
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
            let request = InstallCloneRequest {
                name,
                version,
                tarball_url,
                target,
                parent: parent_path,
            };
            if let Err(e) = clone_scheduler.prefetch_clone(request) {
                tracing::debug!("Pipeline clone prefetch failed: {e:#}");
            }
        }
    });

    PipelineHandles {
        download_handle,
        clone_handle,
    }
}
