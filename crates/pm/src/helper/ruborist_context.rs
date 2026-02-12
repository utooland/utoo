//! Adapter layer bridging pm's configuration to ruborist's API.

use std::path::{Path, PathBuf};
use utoo_ruborist::service::{BuildDepsOptions, Glob, UnifiedRegistry};

use crate::service::pipeline::{PipelineChannels, PipelineReceiver};
use crate::util::cache::get_cache_dir;
use crate::util::logger::ProgressReceiver;
use crate::util::user_config::{
    get_legacy_peer_deps, get_manifests_concurrency_limit, get_registry, get_supports_semver,
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
    /// Create BuildDepsOptions with a custom event receiver.
    pub async fn deps_options<R: utoo_ruborist::progress::EventReceiver>(
        cwd: PathBuf,
        receiver: R,
    ) -> BuildDepsOptions<GlobImpl, R> {
        BuildDepsOptions {
            cwd,
            registry_url: get_registry(),
            cache_dir: Some(get_cache_dir()),
            concurrency: get_manifests_concurrency_limit().await,
            legacy_peer_deps: get_legacy_peer_deps().await,
            glob: TokioGlob,
            receiver,
            supports_semver: get_supports_semver(),
        }
    }

    /// Create BuildDepsOptions with PipelineReceiver for concurrent download/clone.
    /// Returns (options, channels) where channels are used to start pipeline workers.
    pub async fn pipeline_deps_options(
        cwd: PathBuf,
    ) -> (
        BuildDepsOptions<GlobImpl, PipelineReceiver<ProgressReceiver>>,
        PipelineChannels,
    ) {
        let (receiver, channels) = PipelineReceiver::new(ProgressReceiver);
        let options = Self::deps_options(cwd, receiver).await;
        (options, channels)
    }

    /// Resolve dependency tree with plain ProgressReceiver. Returns PackageLock.
    pub async fn build_deps(cwd: PathBuf) -> anyhow::Result<utoo_ruborist::lock::PackageLock> {
        let options = Self::deps_options(cwd, ProgressReceiver).await;
        utoo_ruborist::service::build_deps(options).await
    }

    /// Create a UnifiedRegistry with standard configuration.
    pub fn registry() -> Registry {
        let mut builder = UnifiedRegistry::builder()
            .registry(get_registry())
            .cache_dir(get_cache_dir());
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
