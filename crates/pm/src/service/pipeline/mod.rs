//! Pipeline installer for concurrent manifest resolution and tarball downloading.
//!
//! This module implements a pipeline architecture similar to bun's approach:
//! - Manifest fetching and tarball downloading happen concurrently
//! - When a package is resolved, its tarball download starts immediately
//! - The install scheduler owns inflight dedupe and shares results across phases

mod receiver;

pub use receiver::PipelineReceiver;

use crate::service::install_scheduler::InstallScheduler;
use crate::util::cloner::clone_count;
use crate::util::downloader::download_stats;

/// Print pipeline summary stats.
pub fn print_pipeline_summary() {
    tracing::debug!(
        "Pipeline stats: downloaded={}, cloned={}",
        download_stats().downloaded,
        clone_count(),
    );
}

/// Result of pipeline-based dependency resolution.
pub struct PipelineResult {
    pub package_lock: utoo_ruborist::lock::PackageLock,
}

/// Resolve dependencies with pipeline: concurrent download/clone during resolution.
///
/// Creates PipelineReceiver via Context, runs build_deps, saves lock file.
/// Note: caller is responsible for managing progress bar lifecycle.
pub async fn resolve_with_pipeline(
    root_path: &std::path::Path,
    scheduler: InstallScheduler,
) -> anyhow::Result<PipelineResult> {
    use crate::helper::lock::save_package_lock;
    use crate::helper::ruborist_context::{Context, spawn_save_project_cache};

    let options = Context::pipeline_deps_options(root_path.to_path_buf(), scheduler).await;

    let output = utoo_ruborist::service::build_deps(options).await?;

    save_package_lock(root_path, &output.lock).await?;
    spawn_save_project_cache(root_path.to_path_buf(), output.project_cache);

    Ok(PipelineResult {
        package_lock: output.lock,
    })
}
