//! Pipeline installer for concurrent manifest resolution and tarball downloading.
//!
//! This module implements a pipeline architecture similar to bun's approach:
//! - Manifest fetching and tarball downloading happen concurrently
//! - When a package is resolved, its tarball download starts immediately
//! - Uses global OnceMap to deduplicate requests and share results across phases

mod receiver;
mod worker;

pub use receiver::{PipelineChannels, PipelineReceiver};
pub use worker::PipelineHandles;

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
    pub handles: PipelineHandles,
}

/// Resolve dependencies with pipeline: concurrent download/clone during resolution.
///
/// Creates PipelineReceiver via Context, starts workers, runs build_deps, saves lock file.
/// Returns both the lock and worker handles for the caller to await after install.
/// Note: caller is responsible for managing progress bar lifecycle.
pub async fn resolve_with_pipeline(root_path: &std::path::Path) -> anyhow::Result<PipelineResult> {
    use crate::helper::lock::save_package_lock;
    use crate::helper::ruborist_context::{Context, spawn_save_project_cache};

    let (options, channels) = Context::pipeline_deps_options(root_path.to_path_buf()).await;
    let handles = worker::start_workers(channels, root_path.to_path_buf());

    // `UTOO_RESOLVE=mb` reroutes install through the experimental
    // mb-style fetch path. Pipeline workers are still started, but
    // because mb_fetch doesn't emit `PackageResolved` events, the
    // pipeline only fires once BFS completes (graph_to_package_lock
    // emits `PackagePlaced` from BFS). Install becomes
    // phase-sequential — fetch all manifests, then download +
    // clone. Useful for A/B benchmarking the resolve phase in
    // isolation; the pipelining advantage of the default path is
    // lost.
    let use_mb = std::env::var("UTOO_RESOLVE").as_deref() == Ok("mb");
    let output = if use_mb {
        tracing::debug!("UTOO_RESOLVE=mb: routing install resolve to build_deps_mb");
        utoo_ruborist::service::build_deps_mb(options).await?
    } else {
        utoo_ruborist::service::build_deps(options).await?
    };

    save_package_lock(root_path, &output.lock).await?;
    spawn_save_project_cache(root_path.to_path_buf(), output.project_cache);

    Ok(PipelineResult {
        package_lock: output.lock,
        handles,
    })
}
