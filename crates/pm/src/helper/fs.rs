//! Tokio-based glob implementation for ruborist's Glob trait.

use anyhow::Result;
use std::path::{Path, PathBuf};
use utoo_ruborist::service::{BuildDepsOptions, Glob, UnifiedRegistry};

use crate::util::cache::get_cache_dir;
use crate::util::config::{get_legacy_peer_deps, get_manifests_concurrency_limit, get_registry};
use crate::util::logger::ProgressReceiver;

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
pub(crate) type DepsOptions = BuildDepsOptions<GlobImpl, ProgressReceiver>;

/// Context for ruborist operations.
/// Centralizes Glob and configuration to avoid spreading concrete types.
pub(crate) struct Context;

impl Context {
    /// Create BuildDepsOptions with standard configuration.
    pub async fn build_deps_options(cwd: PathBuf) -> DepsOptions {
        BuildDepsOptions {
            cwd,
            registry_url: get_registry(),
            cache_dir: Some(get_cache_dir()),
            concurrency: get_manifests_concurrency_limit().await,
            legacy_peer_deps: get_legacy_peer_deps().await,
            glob: TokioGlob,
            receiver: ProgressReceiver,
        }
    }

    /// Create a UnifiedRegistry with standard configuration.
    pub fn registry() -> Result<Registry> {
        UnifiedRegistry::builder()
            .registry(get_registry())
            .cache_dir(get_cache_dir())
            .build()
    }

    /// Get the glob instance.
    pub fn glob() -> GlobImpl {
        TokioGlob
    }
}
