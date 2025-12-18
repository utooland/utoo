//! Tokio-based file system implementation for ruborist's FileSystem trait.

use std::path::{Path, PathBuf};
use utoo_ruborist::service::{BuildDepsOptions, FileSystem, Glob, UnifiedRegistry};

use crate::util::cache::get_cache_dir;
use crate::util::config::{get_legacy_peer_deps, get_manifests_concurrency_limit, get_registry};
use crate::util::logger::ProgressReceiver;

/// Tokio-based file system implementation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokioFileSystem;

impl FileSystem for TokioFileSystem {
    type Error = std::io::Error;

    async fn read(&self, path: &Path) -> Result<Vec<u8>, Self::Error> {
        tokio::fs::read(path).await
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, Self::Error> {
        tokio::fs::read_to_string(path).await
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<(), Self::Error> {
        tokio::fs::write(path, content).await
    }

    async fn exists(&self, path: &Path) -> Result<bool, Self::Error> {
        tokio::fs::try_exists(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), Self::Error> {
        tokio::fs::create_dir_all(path).await
    }
}

impl Glob for TokioFileSystem {
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

// Type aliases to hide concrete FileSystem type
pub(crate) type Fs = TokioFileSystem;
pub(crate) type Registry = UnifiedRegistry<Fs>;
pub(crate) type DepsOptions = BuildDepsOptions<Fs, ProgressReceiver>;

/// Context for ruborist operations.
/// Centralizes FileSystem and configuration to avoid spreading concrete types.
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
            fs: TokioFileSystem,
            receiver: ProgressReceiver,
        }
    }

    /// Create a UnifiedRegistry with standard configuration.
    pub fn registry() -> Registry {
        UnifiedRegistry::builder(TokioFileSystem)
            .registry(get_registry())
            .cache_dir(get_cache_dir())
            .build()
    }

    /// Get the file system instance.
    pub fn fs() -> Fs {
        TokioFileSystem
    }
}
