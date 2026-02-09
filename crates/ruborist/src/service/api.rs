//! High-level API for dependency resolution.
//!
//! This module provides a simple, unified API for resolving dependencies
//! that works across different platforms (native CLI, WASM browser).
//!
//! # Example
//!
//! ```ignore
//! use utoo_ruborist::service::{build_deps, BuildDepsOptions, NoopFileSystem};
//! use utoo_ruborist::progress::NoopReceiver;
//!
//! let package_lock = build_deps(BuildDepsOptions {
//!     cwd: PathBuf::from("."),
//!     registry_url: "https://registry.npmmirror.com".to_string(),
//!     cache_dir: None,
//!     concurrency: 20,
//!     legacy_peer_deps: false,
//!     fs: NoopFileSystem,
//!     receiver: NoopReceiver,
//! }).await?;
//!
//! // Serialize to JSON
//! let json = serde_json::to_string_pretty(&package_lock)?;
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use super::cache::{PackageCache, load_project_cache, save_project_cache};
use super::fs::Glob;
use super::registry::UnifiedRegistry;
use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::model::package_lock::{LockPackage, PackageLock};
use crate::model::util::parse_package_spec;
use crate::resolver::builder::{BuildDepsConfig, add_edges_from, build_deps_with_config};
use crate::resolver::runtime::install_runtime_from_map;
use crate::resolver::workspace::WorkspaceDiscovery;
use crate::traits::progress::EventReceiver;

/// Options for dependency resolution.
#[derive(Debug)]
pub struct BuildDepsOptions<G, R> {
    /// Current working directory (contains package.json)
    pub cwd: PathBuf,
    /// Registry URL (e.g., "https://registry.npmmirror.com")
    pub registry_url: String,
    /// Cache directory for disk cache (None = pure in-memory mode)
    pub cache_dir: Option<PathBuf>,
    /// Maximum concurrent network requests (cap for adaptive concurrency)
    pub concurrency: usize,
    /// Whether to skip peer dependencies (legacy mode)
    pub legacy_peer_deps: bool,
    /// Whether to use adaptive concurrency control (AIMD) for preloading
    pub adaptive_concurrency: bool,
    /// Glob implementation for workspace discovery
    pub glob: G,
    /// Progress event receiver
    pub receiver: R,
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
            concurrency: 20,
            legacy_peer_deps: true,
            adaptive_concurrency: true,
            glob,
            receiver,
        }
    }
}

/// Build dependency tree and return PackageLock.
///
/// This is the main entry point for dependency resolution. It:
/// 1. Reads package.json from cwd (and finds workspace root if applicable)
/// 2. Discovers and adds workspace packages
/// 3. Injects runtime dependencies (node-bin packages from engines.install-node)
/// 4. Initializes the dependency graph
/// 5. Resolves all dependencies using the registry
/// 6. Returns a PackageLock ready for serialization
///
/// # Arguments
/// * `options` - Configuration options including cwd, registry, cache, etc.
///
/// # Returns
/// * `PackageLock` - The resolved dependency tree
///
/// # Example
/// ```ignore
/// let lock = build_deps(BuildDepsOptions {
///     cwd: PathBuf::from("."),
///     registry_url: "https://registry.npmmirror.com".to_string(),
///     cache_dir: Some(PathBuf::from("~/.cache/nm")),
///     concurrency: 20,
///     legacy_peer_deps: false,
///     glob: TokioGlob,
///     receiver: MyProgressReceiver,
/// }).await?;
/// ```
pub async fn build_deps<G, R>(options: BuildDepsOptions<G, R>) -> Result<PackageLock>
where
    G: Glob + Clone,
    R: EventReceiver,
{
    let BuildDepsOptions {
        cwd,
        registry_url,
        cache_dir,
        concurrency,
        legacy_peer_deps,
        adaptive_concurrency,
        glob,
        receiver,
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
                pkg.optional_dependencies.entry(name).or_insert(version);
            }
        }
    }

    // 4. Initialize dependency graph
    let mut graph = DependencyGraph::from_package_json(root_path.clone(), pkg.clone());

    // 5. Add root dependency edges
    let root_index = graph.root_index;
    add_edges_from(&mut graph, root_index, &pkg, legacy_peer_deps, true);

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
            ws_pkg.version.clone(),
            EdgeType::Prod,
        );
        graph.mark_dependency_resolved(dep_edge_id, workspace_index);

        tracing::debug!(
            "Added workspace: {} at {:?}",
            workspace.name,
            workspace.path
        );

        // Add workspace dependencies
        add_edges_from(&mut graph, workspace_index, &ws_pkg, legacy_peer_deps, true);
    }

    // 7. Create package cache (with optional disk cache)
    let package_cache = if let Some(cache_path) = &cache_dir {
        Arc::new(PackageCache::with_cache_dir(cache_path.clone()))
    } else {
        Arc::new(PackageCache::new())
    };

    // 8. Load project cache and pre-populate memory cache
    let project_cache_path = root_path.join("node_modules/.utoo-manifest.json");
    let project_cache_data = load_project_cache(&project_cache_path)
        .await
        .unwrap_or_default();

    // Pre-populate cache from project cache
    // Use specs to get original spec -> version mapping, then fetch manifest by version
    let mut cache_count = 0;
    let mut missing_count = 0;
    for (name, pkg_cache) in &project_cache_data.cache {
        for (spec, version) in &pkg_cache.specs {
            if let Some(manifest) = pkg_cache.manifests.get(version) {
                // Cache key is "name@spec" (e.g., "lodash@^4.17.0")
                package_cache.set_version_manifest(name.clone(), spec.clone(), manifest.clone());
                cache_count += 1;
            } else {
                // Spec points to version but manifest is missing - cache is corrupted
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
    if missing_count > 0 {
        tracing::warn!(
            "Project cache has {} specs with missing manifests, will re-fetch from registry",
            missing_count
        );
    }

    if cache_count > 0 {
        tracing::debug!("Loaded {} manifests from project cache", cache_count);
    }

    // 9. Create registry client with shared cache
    let registry = UnifiedRegistry::builder()
        .registry(&registry_url)
        .cache(package_cache)
        .build();

    tracing::debug!(
        "Using registry: {} (semver: {}, disk cache: {})",
        registry.registry_url(),
        registry.supports_semver(),
        cache_dir.is_some()
    );

    // 10. Build dependency tree
    // Skip preload if project cache exists (cache is already warm)
    let skip_preload = cache_count > 0;
    let config = BuildDepsConfig::default()
        .with_legacy_peer_deps(legacy_peer_deps)
        .with_concurrency(concurrency)
        .with_skip_preload(skip_preload)
        .with_adaptive_concurrency(adaptive_concurrency);

    if skip_preload {
        tracing::debug!(
            "Skipping preload phase (project cache has {} entries)",
            cache_count
        );
    }

    build_deps_with_config(&mut graph, &registry, config, &receiver)
        .await
        .map_err(|e| anyhow::anyhow!("Dependency resolution failed: {}", e))?;

    // 11. Serialize to PackageLock
    let (packages_value, _total) = graph.serialize_to_packages(&root_path);
    let packages: HashMap<String, LockPackage> = serde_json::from_value(packages_value)
        .context("Failed to convert packages to lock format")?;

    // 12. Save project cache (export from memory cache)
    let mut new_cache_data = super::cache::ProjectCacheData::default();
    // Export version manifests from memory cache to project cache
    // Memory cache key format: "name@spec", manifest contains resolved version
    for (key, manifest) in registry.cache().export_version_manifests() {
        // Use parse_package_spec to handle scoped packages correctly
        // e.g., "@babel/core@^7.0.0" -> ("@babel/core", "^7.0.0")
        let (name, spec) = parse_package_spec(&key);
        let version = manifest.version.clone();
        let pkg_cache = new_cache_data.cache.entry(name.to_string()).or_default();
        // specs: spec -> version (e.g., "^2.1.1" -> "2.1.1")
        pkg_cache.specs.insert(spec.to_string(), version.clone());
        // manifests: version -> manifest (e.g., "2.1.1" -> {...})
        pkg_cache.manifests.insert(version, manifest);
    }

    if !new_cache_data.cache.is_empty()
        && let Err(e) = save_project_cache(&project_cache_path, &new_cache_data).await
    {
        tracing::warn!("Failed to save project cache: {}", e);
    }

    Ok(PackageLock::new(
        pkg.name.clone(),
        pkg.version.clone(),
        packages,
    ))
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
            concurrency: 20,
            legacy_peer_deps: true,
            adaptive_concurrency: true,
            glob: NoopGlob,
            receiver: NoopReceiver,
        };

        assert_eq!(options.concurrency, 20);
        assert!(options.legacy_peer_deps);
        assert!(options.adaptive_concurrency);
    }

    #[test]
    fn test_project_cache_export_scoped_packages() {
        // Test that scoped packages are correctly parsed when exporting project cache
        // This ensures "@babel/core@^7.0.0" is parsed as ("@babel/core", "^7.0.0")
        // not ("", "babel/core@^7.0.0")

        // Test parse_package_spec directly to avoid polluting global cache
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

        // Verify the old buggy behavior would have failed
        // split_once('@') on "@babel/core@^7.0.0" returns ("", "babel/core@^7.0.0")
        let buggy_result = "@babel/core@^7.0.0".split_once('@');
        assert_eq!(buggy_result, Some(("", "babel/core@^7.0.0")));
    }
}
