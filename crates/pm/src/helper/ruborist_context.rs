//! Adapter layer bridging pm's configuration to ruborist's API.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use utoo_ruborist::service::{
    BuildDepsOptions, BuildDepsOutput, Glob, ManifestStore, UnifiedRegistry,
};

use crate::service::pipeline::{PipelineChannels, PipelineReceiver};
use crate::util::cache::get_cache_dir;
use crate::util::logger::ProgressReceiver;
use crate::util::manifest_store::DiskManifestStore;
use crate::util::project_cache;
use crate::util::user_config::{
    get_catalogs, get_manifests_concurrency_limit, get_peer_deps, get_registry, get_supports_semver,
};

/// Tokio-based glob implementation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokioGlob;

impl Glob for TokioGlob {
    type Error = std::io::Error;

    async fn glob(&self, pattern: &Path) -> Result<Vec<PathBuf>, Self::Error> {
        let pattern_str = pattern.to_string_lossy();
        let paths = glob::glob(&pattern_str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?
            .filter_map(|entry| entry.ok())
            .collect();
        Ok(paths)
    }
}

// Type aliases to hide concrete Glob type
pub(crate) type GlobImpl = TokioGlob;
pub(crate) type Registry = UnifiedRegistry;
/// Context for ruborist operations.
/// Centralizes Glob and configuration to avoid spreading concrete types.
pub(crate) struct Context;

impl Context {
    fn manifest_store() -> Arc<dyn ManifestStore> {
        Arc::new(DiskManifestStore::new(get_cache_dir()))
    }

    /// Create BuildDepsOptions with a custom event receiver.
    pub async fn deps_options<R: utoo_ruborist::progress::EventReceiver>(
        cwd: PathBuf,
        receiver: R,
    ) -> BuildDepsOptions<GlobImpl, R> {
        let catalogs = get_catalogs().await;
        let warm_project_cache = Some(project_cache::load(&cwd).await);
        BuildDepsOptions {
            cwd,
            registry_url: get_registry(),
            cache_dir: Some(get_cache_dir()),
            manifest_store: Self::manifest_store(),
            warm_project_cache,
            concurrency: get_manifests_concurrency_limit().await,
            peer_deps: get_peer_deps().await,
            glob: TokioGlob,
            receiver,
            supports_semver: get_supports_semver(),
            catalogs,
            skip_preload: false,
        }
    }

    /// Create BuildDepsOptions with PipelineReceiver for concurrent download/clone.
    /// Returns (options, channels) where channels are used to start pipeline workers.
    ///
    /// Sets `skip_preload=true` so ruborist's `service::api::build_deps`
    /// routes through `mb_fetch_with_graph` (folded preload + graph
    /// build). The pipeline still receives `PackageResolved` /
    /// `PackagePlaced` events — emitted from inside
    /// `mb_fetch_with_graph` (main loop and graph worker
    /// respectively) — so download/clone start as early as the
    /// classic preload+BFS path.
    pub async fn pipeline_deps_options(
        cwd: PathBuf,
    ) -> (
        BuildDepsOptions<GlobImpl, PipelineReceiver<ProgressReceiver>>,
        PipelineChannels,
    ) {
        let (receiver, channels) = PipelineReceiver::new(ProgressReceiver);
        let mut options = Self::deps_options(cwd, receiver).await;
        options.skip_preload = true;
        (options, channels)
    }

    /// Resolve dependency tree with plain ProgressReceiver. Returns
    /// [`BuildDepsOutput`] (lock + project cache); the project cache is
    /// persisted in the background.
    ///
    /// Used by the lockfile-only path (`utoo deps`). With
    /// `skip_preload=true`, ruborist's `service::api::build_deps`
    /// internally routes through `mb_resolve::mb_fetch` — a
    /// standalone manifest-bench-style preload that bypasses
    /// `service::http` / `service::manifest` / `service::registry`
    /// for the cold-cache lockfile-only workload. PM doesn't see
    /// the dispatch.
    pub async fn build_deps(cwd: PathBuf) -> anyhow::Result<BuildDepsOutput> {
        let mut options = Self::deps_options(cwd.clone(), ProgressReceiver).await;
        options.skip_preload = true;
        let output = utoo_ruborist::service::build_deps(options).await?;
        spawn_save_project_cache(cwd, output.project_cache.clone());
        Ok(output)
    }

    /// Create a UnifiedRegistry with standard configuration.
    pub fn registry() -> Registry {
        let mut builder = UnifiedRegistry::builder()
            .registry(get_registry())
            .store(Self::manifest_store());
        if let Some(semver) = get_supports_semver() {
            builder = builder.supports_semver(semver);
        }
        builder.build()
    }

    /// Get the glob instance.
    pub fn glob() -> GlobImpl {
        TokioGlob
    }
}

/// Persist the project cache in the background — fire-and-forget so the
/// resolver doesn't pay write latency on its critical path.
pub(crate) fn spawn_save_project_cache(
    cwd: PathBuf,
    data: utoo_ruborist::service::ProjectCacheData,
) {
    if data.cache.is_empty() {
        return;
    }
    tokio::spawn(async move {
        if let Err(e) = project_cache::save(&cwd, &data).await {
            tracing::warn!("Failed to save project cache: {e}");
        }
    });
}
