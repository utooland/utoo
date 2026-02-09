//! Pipeline installer for concurrent manifest resolution and tarball downloading.
//!
//! This module implements a pipeline architecture similar to bun's approach:
//! - Manifest fetching and tarball downloading happen concurrently
//! - When a package is resolved, its tarball download starts immediately
//! - Uses global OnceMap to deduplicate requests and share results across phases

mod receiver;
mod worker;

pub use receiver::PipelineReceiver;
pub use worker::PipelineHandles;

use crate::util::cloner::clone_count;
use crate::util::downloader::download_count;

/// Print pipeline summary stats.
pub fn print_pipeline_summary() {
    tracing::info!(
        "Pipeline stats: downloaded={}, cloned={}",
        download_count(),
        clone_count(),
    );
}

/// Result of pipeline-based dependency resolution.
pub struct PipelineResult {
    pub package_lock: utoo_ruborist::lock::PackageLock,
    pub handles: PipelineHandles,
}

/// Resolve dependencies with pipeline: concurrent download/clone during resolution.
///
/// Creates PipelineReceiver, starts workers, runs build_deps, saves lock file.
/// Returns both the lock and worker handles for the caller to await after install.
pub async fn resolve_with_pipeline(root_path: &std::path::Path) -> anyhow::Result<PipelineResult> {
    use crate::helper::lock::save_package_lock;
    use crate::helper::ruborist_context::Context;
    use crate::util::logger::{ProgressReceiver, finish_progress_bar, start_progress_bar};

    start_progress_bar();

    let (receiver, channels) = PipelineReceiver::new(ProgressReceiver);
    let options = Context::deps_options(root_path.to_path_buf(), receiver).await;
    let handles = worker::start_workers(channels, root_path.to_path_buf());

    let package_lock = utoo_ruborist::service::build_deps(options).await?;

    finish_progress_bar("package-lock.json resolved");
    save_package_lock(root_path, &package_lock).await?;

    Ok(PipelineResult {
        package_lock,
        handles,
    })
}
