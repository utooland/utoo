//! Adapter layer bridging pm's configuration to ruborist's API.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::service::{BuildDepsOptions, BuildDepsOutput, Glob, UnifiedRegistry};
use utoo_ruborist::workspace::WorkspaceDiscovery;

use crate::fs;
use crate::service::auth;
use crate::service::install_scheduler::{InstallEventReceiver, InstallScheduler};
use crate::util::cache::get_cache_dir;
use crate::util::json::load_package_lock_json_from_path;
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
    /// Create BuildDepsOptions with a custom event receiver.
    pub async fn deps_options<R: utoo_ruborist::progress::EventReceiver>(
        cwd: PathBuf,
        receiver: R,
    ) -> BuildDepsOptions<GlobImpl, R> {
        let catalogs = get_catalogs().await;

        // Reuse an existing `package-lock.json` as the resolved-tree baseline:
        // the resolver seeds the prior tree and resolves only the delta, so
        // `ut install <pkg>` adds a node instead of recomputing (and
        // reshuffling) the whole tree. The lockfile pins every resolved
        // manifest, so it supersedes the `.utoo-manifest.json` project cache —
        // only fall back to that cache when there is no usable lockfile (a fresh
        // project, or a global install with no lock).
        let baseline = load_baseline(&cwd).await;
        let project_cache = if baseline.is_some() {
            None
        } else {
            Some(project_cache::load(&cwd).await)
        };

        BuildDepsOptions {
            cwd,
            registry: Self::registry().await,
            cache_dir: Some(get_cache_dir()),
            project_cache,
            concurrency: get_manifests_concurrency_limit().await,
            peer_deps: get_peer_deps().await,
            glob: TokioGlob,
            receiver,
            catalogs,
            baseline,
        }
    }

    /// Create BuildDepsOptions that forwards package events to the install scheduler.
    pub async fn install_deps_options(
        cwd: PathBuf,
        scheduler: InstallScheduler,
    ) -> BuildDepsOptions<GlobImpl, InstallEventReceiver<ProgressReceiver>> {
        let receiver = InstallEventReceiver::new(ProgressReceiver, scheduler);
        Self::deps_options(cwd, receiver).await
    }

    /// Resolve dependency tree with plain ProgressReceiver. Returns
    /// [`BuildDepsOutput`] (lock + project cache); the project cache is
    /// persisted in the background.
    pub async fn build_deps(cwd: PathBuf) -> anyhow::Result<BuildDepsOutput> {
        let (root_path, pkg) =
            utoo_ruborist::service::read_root_manifest(&cwd, Self::glob()).await?;
        let options = Self::deps_options(root_path.clone(), ProgressReceiver).await;
        let output = utoo_ruborist::service::build_deps(options, pkg).await?;
        spawn_save_project_cache(root_path, output.project_cache.clone());
        Ok(output)
    }

    /// Create a UnifiedRegistry with standard configuration, including the
    /// private-registry auth token (resolved once, cached). This is the single
    /// place registry config + credentials are assembled; `deps_options` reuses
    /// it so install and `resolve_package` share one authenticated client.
    pub async fn registry() -> Registry {
        let mut builder = UnifiedRegistry::builder()
            .registry(get_registry())
            .auth_token(auth::cached_token().await)
            .store(Arc::new(DiskManifestStore::new(get_cache_dir())));
        if let Some(semver) = get_supports_semver() {
            builder = builder.supports_semver(semver);
        }
        builder.build()
    }

    /// Get the glob instance.
    pub fn glob() -> GlobImpl {
        TokioGlob
    }

    /// Workspace discovery wired to the native glob. The single entry point for
    /// `find_workspaces` / `find_root_path` / `find_project_path`; callers
    /// consume `WorkspacePackage` directly.
    pub fn discovery() -> WorkspaceDiscovery<GlobImpl> {
        WorkspaceDiscovery::new(Self::glob())
    }
}

/// Load an existing `package-lock.json` to reuse as the resolver baseline.
///
/// Returns `None` when no lockfile exists (a fresh project, or a global install)
/// or when it can't be parsed (an old `lockfileVersion: 1` file with no
/// `packages` map, or a corrupt one) — in every such case the caller falls back
/// to a full cold resolve, so an unusable lockfile only costs a warning.
async fn load_baseline(cwd: &Path) -> Option<PackageLock> {
    let lock_path = cwd.join("package-lock.json");
    if !fs::try_exists(&lock_path).await.unwrap_or(false) {
        return None;
    }
    match load_package_lock_json_from_path(cwd).await {
        Ok(lock) => Some(lock),
        Err(e) => {
            tracing::warn!("ignoring package-lock.json for reuse (will cold-resolve): {e:#}");
            None
        }
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
