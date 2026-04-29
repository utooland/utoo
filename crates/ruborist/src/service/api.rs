//! High-level API for dependency resolution.
//!
//! This module provides a simple, unified API for resolving dependencies
//! that works across different platforms (native CLI, WASM browser).
//!
//! # Example
//!
//! ```ignore
//! use utoo_ruborist::service::{build_deps, BuildDepsOptions};
//! use utoo_ruborist::progress::NoopReceiver;
//!
//! let output = build_deps(BuildDepsOptions::new(
//!     PathBuf::from("."),
//!     my_glob,
//!     NoopReceiver,
//! )).await?;
//!
//! // Serialize to JSON
//! let json = serde_json::to_string_pretty(&output.lock)?;
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use super::cache::{PackageCache, ProjectCacheData};
use super::fs::Glob;
use super::registry::UnifiedRegistry;
use super::store::{ManifestStore, NoopStore};
use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::model::package_lock::PackageLock;
use crate::model::util::parse_package_spec;
use crate::resolver::builder::{
    BuildDepsConfig, DevDeps, EdgeContext, PeerDeps, add_edges_from, build_deps_with_config,
};
use crate::resolver::runtime::install_runtime_from_map;
use crate::resolver::workspace::WorkspaceDiscovery;
use crate::spec::Catalogs;
use crate::traits::progress::EventReceiver;

/// Options for dependency resolution.
pub struct BuildDepsOptions<G, R> {
    /// Current working directory (contains package.json)
    pub cwd: PathBuf,
    /// Registry URL (e.g., "https://registry.npmmirror.com")
    pub registry_url: String,
    /// Tarball cache directory passed through to non-registry resolvers
    /// (http/tarball, native-git). Unrelated to manifest disk cache.
    pub cache_dir: Option<PathBuf>,
    /// Persistence backend for manifest cache. Defaults to `NoopStore`
    /// (everything is in-memory).
    pub manifest_store: Arc<dyn ManifestStore>,
    /// Project-level warm cache pre-loaded by the host. Pre-populates the
    /// in-memory manifest cache to skip the preload phase on a warm install.
    pub warm_project_cache: Option<ProjectCacheData>,
    /// Maximum concurrent network requests
    pub concurrency: usize,
    /// How to handle peer dependencies.
    pub peer_deps: PeerDeps,
    /// Glob implementation for workspace discovery
    pub glob: G,
    /// Progress event receiver
    pub receiver: R,
    /// Explicit semver support override (None = auto-detect from registry URL)
    pub supports_semver: Option<bool>,
    /// Catalog definitions for the `catalog:` dependency protocol.
    /// Key `""` = default catalog, other keys = named catalogs.
    pub catalogs: Catalogs,
}

impl<G, R> BuildDepsOptions<G, R> {
    /// Create options with default values.
    pub fn new(cwd: PathBuf, glob: G, receiver: R) -> Self
    where
        G: Default,
        R: Default,
    {
        Self {
            cwd,
            registry_url: "https://registry.npmmirror.com".to_string(),
            cache_dir: None,
            manifest_store: Arc::new(NoopStore),
            warm_project_cache: None,
            concurrency: 20,
            peer_deps: PeerDeps::Skip,
            glob,
            receiver,
            supports_semver: None,
            catalogs: HashMap::new(),
        }
    }
}

/// Output of [`build_deps`].
///
/// `project_cache` carries the manifests resolved during this run; the host
/// decides whether and where to persist it (typically to
/// `node_modules/.utoo-manifest.json`). It is empty when no resolutions
/// happened.
pub struct BuildDepsOutput {
    pub lock: PackageLock,
    pub project_cache: ProjectCacheData,
}

/// Build dependency tree and return [`BuildDepsOutput`].
///
/// This is the main entry point for dependency resolution. It:
/// 1. Reads package.json from cwd (and finds workspace root if applicable)
/// 2. Discovers and adds workspace packages
/// 3. Injects runtime dependencies (node-bin packages from engines.install-node)
/// 4. Initializes the dependency graph
/// 5. Resolves all dependencies using the registry
/// 6. Returns a [`BuildDepsOutput`] with the package lock and the new project cache
pub async fn build_deps<G, R>(options: BuildDepsOptions<G, R>) -> Result<BuildDepsOutput>
where
    G: Glob + Clone,
    R: EventReceiver,
{
    let BuildDepsOptions {
        cwd,
        registry_url,
        cache_dir,
        manifest_store,
        warm_project_cache,
        concurrency,
        peer_deps,
        glob,
        receiver,
        supports_semver,
        catalogs,
    } = options;

    // 1. Find root path (workspace root if applicable)
    let discovery = WorkspaceDiscovery::new(glob.clone());
    let root_path = discovery.find_root_path(&cwd).await?;

    // 2. Read root package.json
    let pkg_path = root_path.join("package.json");
    let mut pkg: PackageJson = super::fs::read_json(&pkg_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read/parse package.json: {}", e))?;

    // 3. Inject runtime dependencies (node-bin packages)
    if let Some(engines) = &pkg.engines {
        let runtime_deps = install_runtime_from_map(engines);
        if !runtime_deps.is_empty() {
            tracing::debug!("Injecting {} runtime dependencies", runtime_deps.len());
            for (name, version) in runtime_deps {
                pkg.optional_dependencies
                    .get_or_insert_with(HashMap::new)
                    .entry(name)
                    .or_insert(version);
            }
        }
    }

    // 4. Initialize dependency graph
    let mut graph = DependencyGraph::from_package_json(root_path.clone(), pkg.clone());

    // 5. Add root dependency edges (catalog: specs resolved at edge creation)
    let root_index = graph.root_index;
    let edge_ctx = EdgeContext::new(peer_deps, DevDeps::Include).with_catalogs(&catalogs);
    add_edges_from(&mut graph, root_index, &pkg, &edge_ctx);

    // 6. Discover and add workspace packages
    let workspaces = discovery.find_workspaces_from_pkg(&root_path, &pkg).await?;

    for workspace in workspaces {
        let ws_pkg = workspace.package_json;

        // Create workspace node
        let workspace_node =
            PackageNode::workspace_from_package_json(workspace.path.clone(), ws_pkg.clone());
        let workspace_index = graph.add_node(workspace_node);

        // Create link node
        let link_node = PackageNode::link_from_package_json(workspace.path.clone(), ws_pkg.clone());
        let link_index = graph.add_node(link_node);

        // Add physical edges
        graph.add_physical_edge(root_index, workspace_index);
        graph.add_physical_edge(root_index, link_index);

        // Create and mark dependency edge as resolved
        let dep_edge_id = graph.add_dependency_edge(
            root_index,
            workspace.name.clone(),
            &ws_pkg.version,
            EdgeType::Prod,
        );
        graph.mark_dependency_resolved(dep_edge_id, workspace_index);

        tracing::debug!(
            "Added workspace: {} at {:?}",
            workspace.name,
            workspace.path
        );

        // Add workspace dependencies (catalog: specs resolved at edge creation)
        add_edges_from(&mut graph, workspace_index, &ws_pkg, &edge_ctx);
    }

    // 7. Create in-memory package cache.
    let package_cache = Arc::new(PackageCache::new());

    // 8. Pre-populate from warm project cache (host-supplied).
    let mut cache_count = 0;
    let mut missing_count = 0;
    if let Some(ref project_cache_data) = warm_project_cache {
        for (name, pkg_cache) in &project_cache_data.cache {
            for (spec, version) in &pkg_cache.specs {
                if let Some(manifest) = pkg_cache.manifests.get(version) {
                    package_cache.set_version_manifest(
                        name.clone(),
                        spec.clone(),
                        Arc::new(manifest.clone()),
                    );
                    cache_count += 1;
                } else {
                    tracing::debug!(
                        "Project cache missing manifest: {}@{} (version {})",
                        name,
                        spec,
                        version
                    );
                    missing_count += 1;
                }
            }
        }
    }
    if missing_count > 0 {
        tracing::warn!(
            "Project cache has {} specs with missing manifests, will re-fetch from registry",
            missing_count
        );
    }

    if cache_count > 0 {
        tracing::debug!("Loaded {} manifests from project cache", cache_count);
    }

    // 9. Create registry client with shared cache and persistence backend.
    let mut builder = UnifiedRegistry::builder()
        .registry(&registry_url)
        .cache(package_cache)
        .store(Arc::clone(&manifest_store));
    if let Some(semver) = supports_semver {
        builder = builder.supports_semver(semver);
    }
    let registry = builder.build();

    tracing::debug!(
        "Using registry: {} (semver: {})",
        registry.registry_url(),
        registry.supports_semver(),
    );

    // 10. Build dependency tree
    // Skip preload if project cache exists (cache is already warm)
    let skip_preload = cache_count > 0;
    let mut config = BuildDepsConfig::default()
        .with_peer_deps(peer_deps)
        .with_concurrency(concurrency)
        .with_skip_preload(skip_preload)
        .with_catalogs(catalogs);
    if let Some(dir) = cache_dir {
        config = config.with_cache_dir(dir);
    }

    if skip_preload {
        tracing::debug!(
            "Skipping preload phase (project cache has {} entries)",
            cache_count
        );
    }

    // Preserve the typed error via `Error::new` + `.context(...)` so CLI
    // renderers (e.g. pm's format_print) can downcast and pretty-print the
    // dependency chain carried by `ResolveError::WithChain`.
    build_deps_with_config(&mut graph, &registry, config, &receiver)
        .await
        .map_err(|e| anyhow::Error::new(e).context("Dependency resolution failed"))?;

    // 11. Serialize to PackageLock
    let (packages, _total) = graph.serialize_to_packages(&root_path);

    // 12. Export project cache from memory cache.
    let mut project_cache = ProjectCacheData::default();
    for (key, manifest) in registry.cache().export_version_manifests() {
        // Use parse_package_spec to handle scoped packages correctly
        // e.g., "@babel/core@^7.0.0" -> ("@babel/core", "^7.0.0")
        let (name, spec) = parse_package_spec(&key);
        let version = manifest.version.clone();
        let pkg_cache = project_cache.cache.entry(name.to_string()).or_default();
        pkg_cache.specs.insert(spec.to_string(), version.clone());
        pkg_cache.manifests.insert(version, (*manifest).clone());
    }

    Ok(BuildDepsOutput {
        lock: PackageLock::new(&pkg.name, &pkg.version, packages),
        project_cache,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::fs::NoopGlob;
    use crate::traits::progress::NoopReceiver;

    #[tokio::test]
    async fn test_build_deps_options_creation() {
        let options: BuildDepsOptions<NoopGlob, NoopReceiver> = BuildDepsOptions {
            cwd: PathBuf::from("."),
            registry_url: "https://registry.npmmirror.com".to_string(),
            cache_dir: None,
            manifest_store: Arc::new(NoopStore),
            warm_project_cache: None,
            concurrency: 20,
            peer_deps: PeerDeps::Skip,
            glob: NoopGlob,
            receiver: NoopReceiver,
            supports_semver: None,
            catalogs: HashMap::new(),
        };

        assert_eq!(options.concurrency, 20);
        assert_eq!(options.peer_deps, PeerDeps::Skip);
        assert!(options.supports_semver.is_none());
    }

    #[test]
    fn test_project_cache_export_scoped_packages() {
        // Test that scoped packages are correctly parsed when exporting project cache
        // This ensures "@babel/core@^7.0.0" is parsed as ("@babel/core", "^7.0.0")
        // not ("", "babel/core@^7.0.0")
        let test_cases = [
            // (input, expected_name, expected_spec)
            ("@babel/core@^7.0.0", "@babel/core", "^7.0.0"),
            ("@types/node@^18.0.0", "@types/node", "^18.0.0"),
            ("@scope/pkg@1.0.0", "@scope/pkg", "1.0.0"),
            ("lodash@^4.17.0", "lodash", "^4.17.0"),
            ("express@4.18.0", "express", "4.18.0"),
        ];

        for (input, expected_name, expected_spec) in test_cases {
            let (name, spec) = crate::model::util::parse_package_spec(input);
            assert_eq!(
                name, expected_name,
                "Failed for input '{}': expected name '{}', got '{}'",
                input, expected_name, name
            );
            assert_eq!(
                spec, expected_spec,
                "Failed for input '{}': expected spec '{}', got '{}'",
                input, expected_spec, spec
            );
        }

        let buggy_result = "@babel/core@^7.0.0".split_once('@');
        assert_eq!(buggy_result, Some(("", "babel/core@^7.0.0")));
    }
}
