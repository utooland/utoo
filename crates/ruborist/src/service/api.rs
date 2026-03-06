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

use anyhow::Result;

use super::cache::{PackageCache, load_project_cache, save_project_cache};
use super::fs::Glob;
use super::registry::UnifiedRegistry;
use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::node::EdgeType;
use crate::model::package_json::{PackageJson, ResolveCatalogs};
use crate::model::package_lock::PackageLock;
use crate::model::spec::Catalogs;
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
    /// Maximum concurrent network requests
    pub concurrency: usize,
    /// Whether to skip peer dependencies (legacy mode)
    pub legacy_peer_deps: bool,
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
            concurrency: 20,
            legacy_peer_deps: true,
            glob,
            receiver,
            supports_semver: None,
            catalogs: HashMap::new(),
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

    // 3. Resolve catalog: specifiers in root package.json
    pkg.resolve_catalogs(&catalogs);

    // 4. Inject runtime dependencies (node-bin packages)
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

    // 5. Add root dependency edges
    let root_index = graph.root_index;
    add_edges_from(&mut graph, root_index, &pkg, legacy_peer_deps, true);

    // 6. Discover and add workspace packages
    let workspaces = discovery.find_workspaces_from_pkg(&root_path, &pkg).await?;

    for workspace in workspaces {
        let mut ws_pkg = workspace.package_json;

        // Resolve catalog: specifiers in workspace package.json
        ws_pkg.resolve_catalogs(&catalogs);

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
                package_cache.set_version_manifest(
                    name.clone(),
                    spec.clone(),
                    Arc::new(manifest.clone()),
                );
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
    let mut builder = UnifiedRegistry::builder()
        .registry(&registry_url)
        .cache(package_cache);
    if let Some(semver) = supports_semver {
        builder = builder.supports_semver(semver);
    }
    let registry = builder.build();

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
        .with_skip_preload(skip_preload);

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
    let (packages, _total) = graph.serialize_to_packages(&root_path);

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
        pkg_cache.manifests.insert(version, (*manifest).clone());
    }

    if !new_cache_data.cache.is_empty()
        && let Err(e) = save_project_cache(&project_cache_path, &new_cache_data).await
    {
        tracing::warn!("Failed to save project cache: {}", e);
    }

    Ok(PackageLock::new(&pkg.name, &pkg.version, packages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::spec::resolve_catalog_specs;
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
            glob: NoopGlob,
            receiver: NoopReceiver,
            supports_semver: None,
            catalogs: HashMap::new(),
        };

        assert_eq!(options.concurrency, 20);
        assert!(options.legacy_peer_deps);
        assert!(options.supports_semver.is_none());
    }

    #[test]
    fn test_resolve_catalog_specs_default() {
        let mut catalogs: Catalogs = HashMap::new();
        catalogs.insert(
            String::new(),
            HashMap::from([
                ("lodash".to_string(), "^4.17.21".to_string()),
                ("react".to_string(), "^18.0.0".to_string()),
            ]),
        );

        let mut deps = HashMap::from([
            ("lodash".to_string(), "catalog:".to_string()),
            ("express".to_string(), "^4.18.0".to_string()),
        ]);

        resolve_catalog_specs(&mut deps, &catalogs);

        assert_eq!(deps.get("lodash").unwrap(), "^4.17.21");
        // Non-catalog specs are untouched
        assert_eq!(deps.get("express").unwrap(), "^4.18.0");
    }

    #[test]
    fn test_resolve_catalog_specs_explicit_default() {
        let mut catalogs: Catalogs = HashMap::new();
        catalogs.insert(
            String::new(),
            HashMap::from([("typescript".to_string(), "^5.0.0".to_string())]),
        );

        let mut deps = HashMap::from([("typescript".to_string(), "catalog:default".to_string())]);

        resolve_catalog_specs(&mut deps, &catalogs);

        assert_eq!(deps.get("typescript").unwrap(), "^5.0.0");
    }

    #[test]
    fn test_resolve_catalog_specs_named() {
        let mut catalogs: Catalogs = HashMap::new();
        catalogs.insert(
            "legacy".to_string(),
            HashMap::from([("express".to_string(), "^3.0.0".to_string())]),
        );

        let mut deps = HashMap::from([("express".to_string(), "catalog:legacy".to_string())]);

        resolve_catalog_specs(&mut deps, &catalogs);

        assert_eq!(deps.get("express").unwrap(), "^3.0.0");
    }

    #[test]
    fn test_resolve_catalog_specs_missing_catalog() {
        let catalogs: Catalogs = HashMap::new();
        let mut deps = HashMap::from([("lodash".to_string(), "catalog:".to_string())]);

        resolve_catalog_specs(&mut deps, &catalogs);

        // Left as-is when catalog not found
        assert_eq!(deps.get("lodash").unwrap(), "catalog:");
    }

    #[test]
    fn test_resolve_catalog_specs_missing_package() {
        let mut catalogs: Catalogs = HashMap::new();
        catalogs.insert(
            String::new(),
            HashMap::from([("react".to_string(), "^18.0.0".to_string())]),
        );

        let mut deps = HashMap::from([("lodash".to_string(), "catalog:".to_string())]);

        resolve_catalog_specs(&mut deps, &catalogs);

        // Left as-is when package not in catalog
        assert_eq!(deps.get("lodash").unwrap(), "catalog:");
    }

    #[test]
    fn test_resolve_catalog_specs_empty_catalogs_noop() {
        let catalogs: Catalogs = HashMap::new();
        let mut deps = HashMap::from([("lodash".to_string(), "^4.17.21".to_string())]);

        resolve_catalog_specs(&mut deps, &catalogs);

        assert_eq!(deps.get("lodash").unwrap(), "^4.17.21");
    }

    #[test]
    fn test_resolve_catalogs_all_dep_types() {
        let mut catalogs: Catalogs = HashMap::new();
        catalogs.insert(
            String::new(),
            HashMap::from([
                ("lodash".to_string(), "^4.17.21".to_string()),
                ("vitest".to_string(), "^1.0.0".to_string()),
                ("react".to_string(), "^18.0.0".to_string()),
                ("zod".to_string(), "^3.0.0".to_string()),
            ]),
        );

        let mut pkg = PackageJson {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Some(HashMap::from([(
                "lodash".to_string(),
                "catalog:".to_string(),
            )])),
            dev_dependencies: Some(HashMap::from([(
                "vitest".to_string(),
                "catalog:".to_string(),
            )])),
            peer_dependencies: Some(HashMap::from([(
                "react".to_string(),
                "catalog:".to_string(),
            )])),
            optional_dependencies: Some(HashMap::from([(
                "zod".to_string(),
                "catalog:".to_string(),
            )])),
            ..Default::default()
        };

        pkg.resolve_catalogs(&catalogs);

        assert_eq!(
            pkg.dependencies.as_ref().unwrap().get("lodash").unwrap(),
            "^4.17.21"
        );
        assert_eq!(
            pkg.dev_dependencies
                .as_ref()
                .unwrap()
                .get("vitest")
                .unwrap(),
            "^1.0.0"
        );
        assert_eq!(
            pkg.peer_dependencies
                .as_ref()
                .unwrap()
                .get("react")
                .unwrap(),
            "^18.0.0"
        );
        assert_eq!(
            pkg.optional_dependencies
                .as_ref()
                .unwrap()
                .get("zod")
                .unwrap(),
            "^3.0.0"
        );
    }

    #[test]
    fn test_resolve_catalogs_empty_is_noop() {
        let catalogs: Catalogs = HashMap::new();
        let mut pkg = PackageJson {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Some(HashMap::from([(
                "lodash".to_string(),
                "catalog:".to_string(),
            )])),
            ..Default::default()
        };

        pkg.resolve_catalogs(&catalogs);

        // catalog: spec left as-is when catalogs is empty
        assert_eq!(
            pkg.dependencies.as_ref().unwrap().get("lodash").unwrap(),
            "catalog:"
        );
    }

    #[test]
    fn test_resolve_catalog_specs_mixed_default_and_named() {
        let mut catalogs: Catalogs = HashMap::new();
        catalogs.insert(
            String::new(),
            HashMap::from([("debug".to_string(), "^4.3.4".to_string())]),
        );
        catalogs.insert(
            "legacy".to_string(),
            HashMap::from([("debug".to_string(), "^3.2.7".to_string())]),
        );

        let mut deps_default = HashMap::from([("debug".to_string(), "catalog:".to_string())]);
        let mut deps_named = HashMap::from([("debug".to_string(), "catalog:legacy".to_string())]);

        resolve_catalog_specs(&mut deps_default, &catalogs);
        resolve_catalog_specs(&mut deps_named, &catalogs);

        // Same package, different versions from different catalogs
        assert_eq!(deps_default.get("debug").unwrap(), "^4.3.4");
        assert_eq!(deps_named.get("debug").unwrap(), "^3.2.7");
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
