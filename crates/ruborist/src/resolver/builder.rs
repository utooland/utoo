//! Dependency tree builder using BFS traversal.
//!
//! This module provides the core algorithm for building a dependency graph
//! from a root package. It uses breadth-first traversal to resolve dependencies
//! level by level, with support for:
//! - Version conflict detection and nested installation
//! - Hoisting (placing packages as high as possible in the tree)
//! - Override rules
//! - Different dependency types (prod, dev, peer, optional)
//! - Parallel manifest preloading for performance
//!
//! # Two-Phase Resolution
//!
//! The builder uses a two-phase approach for optimal performance:
//! 1. **Preload Phase**: Parallel fetch of all manifests to warm up caches
//! 2. **Build Phase**: Sequential BFS traversal reading from cache
//!
//! This separation allows for maximum parallelism during network I/O
//! while keeping the graph building logic simple and deterministic.

use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::resolver::preload::{PreloadConfig, preload_manifests};
use crate::resolver::registry::{ResolveError, resolve_dependency};
use crate::traits::progress::{BuildEvent, EventReceiver, NoopReceiver};
use crate::traits::registry::{RegistryClient, ResolvedPackage};

// Re-export edge types
pub use super::edges::{
    DependencyEdgeInfo, DependencySource, add_edges_from, collect_unresolved_edges,
};

/// Configuration for dependency resolution.
#[derive(Debug, Clone)]
pub struct BuildDepsConfig {
    /// Whether to skip peer dependencies (legacy mode)
    pub legacy_peer_deps: bool,
    /// Maximum number of concurrent manifest fetches during preload
    pub concurrency: usize,
    /// Whether to skip preload phase (useful when cache is already warm)
    pub skip_preload: bool,
}

impl Default for BuildDepsConfig {
    fn default() -> Self {
        Self {
            legacy_peer_deps: true,
            concurrency: crate::resolver::preload::DEFAULT_CONCURRENCY,
            skip_preload: false,
        }
    }
}

impl BuildDepsConfig {
    /// Create config with legacy peer deps mode
    pub fn with_legacy_peer_deps(mut self, legacy: bool) -> Self {
        self.legacy_peer_deps = legacy;
        self
    }

    /// Create config with custom concurrency
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Create config that skips preload phase
    pub fn with_skip_preload(mut self, skip: bool) -> Self {
        self.skip_preload = skip;
        self
    }
}

/// Snapshot of node dependency flags to avoid borrowing conflicts.
#[derive(Debug, Clone, Copy)]
struct NodeFlags {
    is_root: bool,
    is_prod: bool,
    is_dev: bool,
    is_optional: bool,
    is_peer: bool,
}

/// Gather all unresolved deps from root and workspace nodes for preloading.
fn gather_preload_deps(graph: &DependencyGraph, legacy_peer_deps: bool) -> Vec<(String, String)> {
    use crate::resolver::preload::is_local_spec;
    use std::collections::HashSet;

    let mut deps = HashSet::new();

    // Collect from root node
    for (_, edge) in graph.get_dependency_edges(graph.root_index) {
        if !edge.valid && !is_local_spec(&edge.spec) {
            // Skip peer deps in legacy mode
            if legacy_peer_deps && edge.edge_type == EdgeType::Peer {
                continue;
            }
            deps.insert((edge.name.clone(), edge.spec.clone()));
        }
    }

    // Collect from workspace nodes
    for node_index in graph.graph.node_indices() {
        if let Some(node) = graph.get_node(node_index)
            && node.is_workspace()
        {
            for (_, edge) in graph.get_dependency_edges(node_index) {
                if !edge.valid && !is_local_spec(&edge.spec) {
                    // Skip peer deps in legacy mode
                    if legacy_peer_deps && edge.edge_type == EdgeType::Peer {
                        continue;
                    }
                    deps.insert((edge.name.clone(), edge.spec.clone()));
                }
            }
        }
    }

    deps.into_iter().collect()
}

/// Create a new package node for a resolved dependency.
///
/// # Arguments
/// * `name` - Package name
/// * `pkg` - Resolved package info from registry
/// * `parent` - Parent node index (determines installation path)
/// * `graph` - The dependency graph (for path calculation)
pub fn create_package_node(
    name: &str,
    pkg: &ResolvedPackage,
    parent: NodeIndex,
    graph: &DependencyGraph,
) -> PackageNode {
    let parent_node = graph
        .get_node(parent)
        .expect("Parent node must exist in graph");

    let path = if parent_node.path.to_string_lossy().is_empty()
        || parent_node.path == std::path::Path::new(".")
    {
        PathBuf::from(format!("node_modules/{name}"))
    } else {
        parent_node.path.join(format!("node_modules/{name}"))
    };

    tracing::debug!(
        "Creating node {}@{} under parent {:?} at {}",
        name,
        pkg.version,
        parent,
        path.display()
    );

    PackageNode::from_version_manifest(name.to_string(), path, pkg.manifest.clone())
}

/// Update target node type based on source node and edge type.
///
/// This function propagates dependency types through the graph according to npm rules:
/// - Root dependencies directly set the target node type
/// - Prod dependencies propagate through prod edges
/// - Dev/Optional flags propagate only when appropriate
pub fn update_node_type_from_edge(
    graph: &mut DependencyGraph,
    from_index: NodeIndex,
    to_index: NodeIndex,
    edge_type: &EdgeType,
) {
    // Extract source node information to avoid borrowing conflicts
    let source_flags = {
        let from_node = graph
            .get_node(from_index)
            .expect("Source node must exist in graph");
        NodeFlags {
            is_root: from_node.is_root(),
            is_prod: from_node.is_prod,
            is_dev: from_node.is_dev,
            is_optional: from_node.is_optional,
            is_peer: from_node.is_peer,
        }
    };

    let to_node = graph
        .get_node_mut(to_index)
        .expect("Target node must exist in graph");

    // Root node dependencies directly determine target type
    if source_flags.is_root {
        match edge_type {
            EdgeType::Prod => {
                to_node.is_prod = true;
                to_node.is_dev = false;
                to_node.is_optional = false;
                to_node.is_peer = false;
            }
            EdgeType::Dev => {
                if !to_node.is_prod {
                    to_node.is_dev = true;
                    to_node.is_optional = false;
                }
            }
            EdgeType::Optional => {
                if !to_node.is_prod && !to_node.is_dev {
                    to_node.is_optional = true;
                }
            }
            EdgeType::Peer => {
                if !to_node.is_prod && !to_node.is_dev {
                    to_node.is_peer = true;
                }
            }
        }
    } else {
        // Propagate types from non-root nodes
        // 1. Source's dev status propagates to target
        // 2. If edge is Optional, target gets optional flag (unless already prod)
        if source_flags.is_dev && !to_node.is_prod {
            to_node.is_dev = true;
        }

        // Handle edge type
        if *edge_type == EdgeType::Optional && !to_node.is_prod {
            // Optional edge -> target is optional
            to_node.is_optional = true;
        } else if source_flags.is_prod && *edge_type != EdgeType::Optional {
            // Prod source with non-optional edge -> target becomes prod
            to_node.is_prod = true;
            to_node.is_dev = false;
            to_node.is_optional = false;
            to_node.is_peer = false;
        } else if source_flags.is_optional && !to_node.is_prod {
            // Optional source propagates optional status
            // Note: don't check !is_dev here - devOptional packages should propagate both flags
            to_node.is_optional = true;
        } else if source_flags.is_peer && !to_node.is_prod && !to_node.is_dev {
            to_node.is_peer = true;
        }
    }
}

/// Result of processing a single dependency.
#[derive(Debug)]
pub enum ProcessResult {
    /// Reused an existing node
    Reused(NodeIndex),
    /// Created a new node
    Created(NodeIndex),
    /// Skipped (optional dependency that failed to resolve)
    Skipped,
}

/// Process a single dependency edge.
///
/// This is the core logic for resolving a dependency:
/// 1. Check if an existing compatible version can be reused
/// 2. If not, resolve from registry and create a new node
/// 3. Handle conflicts by installing nested
///
/// # Arguments
/// * `graph` - The dependency graph
/// * `registry` - Registry client for fetching packages
/// * `node_index` - The node that has this dependency
/// * `edge_info` - Information about the dependency edge
/// * `legacy_peer_deps` - If true, skip peer dependencies when adding edges
///
/// # Returns
/// The result of processing (reused, created, or skipped)
pub async fn process_dependency<R: RegistryClient>(
    graph: &mut DependencyGraph,
    registry: &R,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    legacy_peer_deps: bool,
) -> Result<ProcessResult, ResolveError<R::Error>> {
    tracing::debug!(
        "Processing dependency {}@{} from [{:?}]",
        edge_info.name,
        edge_info.spec,
        node_index
    );

    // Find installation location
    match graph.find_compatible_node(node_index, &edge_info.name, &edge_info.spec) {
        FindResult::Reuse(existing_index) => {
            let version = graph
                .get_node(existing_index)
                .map(|n| n.version.as_str())
                .unwrap_or("unknown");
            tracing::debug!(
                "Reusing existing {}@{} at {:?}",
                edge_info.name,
                version,
                existing_index
            );

            // Mark edge as resolved
            graph.mark_dependency_resolved(edge_info.edge_id, existing_index);

            // Update target node type
            update_node_type_from_edge(graph, node_index, existing_index, &edge_info.edge_type);

            Ok(ProcessResult::Reused(existing_index))
        }
        FindResult::Conflict(conflict_parent) | FindResult::New(conflict_parent) => {
            tracing::debug!(
                "Need to resolve {}@{} (conflict/new at {:?})",
                edge_info.name,
                edge_info.spec,
                conflict_parent
            );

            // Resolve from registry first to get the version
            let resolved = match resolve_dependency(
                registry,
                &edge_info.name,
                &edge_info.spec,
                &edge_info.edge_type,
            )
            .await?
            {
                Some(resolved) => resolved,
                None => {
                    tracing::debug!(
                        "Skipped optional dependency {}@{}",
                        edge_info.name,
                        edge_info.spec
                    );
                    return Ok(ProcessResult::Skipped);
                }
            };

            // Check override using resolved version (not original spec)
            let resolved = if let Some(override_spec) =
                graph.check_override(node_index, &edge_info.name, Some(&resolved.version))
            {
                tracing::debug!(
                    "Override: {}@{} (resolved {}) => {}",
                    edge_info.name,
                    edge_info.spec,
                    resolved.version,
                    override_spec
                );
                // Re-resolve with override spec
                match resolve_dependency(
                    registry,
                    &edge_info.name,
                    &override_spec,
                    &edge_info.edge_type,
                )
                .await?
                {
                    Some(r) => r,
                    None => resolved, // Fallback to original if override fails
                }
            } else {
                resolved
            };

            tracing::debug!(
                "Resolved {}@{} => {}",
                edge_info.name,
                edge_info.spec,
                resolved.version
            );

            // Create new node
            let new_node = create_package_node(&edge_info.name, &resolved, conflict_parent, graph);
            let new_index = graph.add_node(new_node);

            // Add physical edge
            graph.add_physical_edge(conflict_parent, new_index);

            // Mark dependency as resolved
            graph.mark_dependency_resolved(edge_info.edge_id, new_index);

            // Update node type
            update_node_type_from_edge(graph, node_index, new_index, &edge_info.edge_type);

            // Add dependencies of the new node
            add_edges_from(
                graph,
                new_index,
                &resolved.manifest,
                legacy_peer_deps,
                false,
            );

            Ok(ProcessResult::Created(new_index))
        }
    }
}

/// Build the complete dependency tree using BFS traversal.
///
/// This is the main entry point for dependency resolution. It starts from
/// the root node and resolves all dependencies level by level.
///
/// # Arguments
/// * `graph` - The dependency graph (should have root node and initial edges)
/// * `registry` - Registry client for fetching packages
/// * `legacy_peer_deps` - If true, skip peer dependencies
///
/// # Example
/// ```ignore
/// let mut graph = DependencyGraph::new(path, package_json);
/// // Add initial dependency edges to root...
/// build_deps(&mut graph, &registry, false).await?;
/// ```
pub async fn build_deps<R: RegistryClient>(
    graph: &mut DependencyGraph,
    registry: &R,
    legacy_peer_deps: bool,
) -> Result<(), ResolveError<R::Error>> {
    let config = BuildDepsConfig::default().with_legacy_peer_deps(legacy_peer_deps);
    build_deps_with_config(graph, registry, config, &NoopReceiver).await
}

/// Build the complete dependency tree with an event receiver.
///
/// Same as `build_deps` but accepts an event receiver for tracking progress
/// and diagnostics. Events are emitted during resolution for:
/// - Progress tracking (CLI progress bars)
/// - Logging and debugging
/// - UI updates (WASM)
///
/// # Arguments
/// * `graph` - The dependency graph (should have root node and initial edges)
/// * `registry` - Registry client for fetching packages
/// * `legacy_peer_deps` - If true, skip peer dependencies
/// * `receiver` - Event receiver for handling build events
pub async fn build_deps_with_receiver<R: RegistryClient, E: EventReceiver>(
    graph: &mut DependencyGraph,
    registry: &R,
    legacy_peer_deps: bool,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>> {
    let config = BuildDepsConfig::default().with_legacy_peer_deps(legacy_peer_deps);
    build_deps_with_config(graph, registry, config, receiver).await
}

/// Build the complete dependency tree with full configuration.
///
/// This is the most flexible entry point for dependency resolution. It performs:
/// 1. **Preload Phase** (unless skipped): Parallel fetch of all manifests to warm up caches
/// 2. **Build Phase**: Sequential BFS traversal reading from cache
///
/// # Arguments
/// * `graph` - The dependency graph (should have root node and initial edges)
/// * `registry` - Registry client for fetching packages
/// * `config` - Build configuration (concurrency, legacy_peer_deps, skip_preload)
/// * `receiver` - Event receiver for handling build events
///
/// # Example
/// ```ignore
/// let config = BuildDepsConfig::default()
///     .with_concurrency(50)
///     .with_skip_preload(true); // Skip preload if cache is warm
///
/// build_deps_with_config(&mut graph, &registry, config, &receiver).await?;
/// ```
pub async fn build_deps_with_config<R: RegistryClient, E: EventReceiver>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: BuildDepsConfig,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>> {
    tracing::debug!(
        "Starting dependency tree build, legacy_peer_deps: {}, concurrency: {}, skip_preload: {}",
        config.legacy_peer_deps,
        config.concurrency,
        config.skip_preload
    );

    // Phase 1: Preload manifests in parallel (unless skipped)
    run_preload_phase(graph, registry, &config, receiver).await;

    // Phase 2: BFS traversal to build the dependency tree
    run_bfs_phase(graph, registry, config.legacy_peer_deps, receiver).await?;

    receiver.on_event(BuildEvent::Complete {
        total_nodes: graph.graph.node_count(),
    });

    Ok(())
}

/// Run the preload phase to warm up the cache with manifests.
async fn run_preload_phase<R: RegistryClient, E: EventReceiver>(
    graph: &DependencyGraph,
    registry: &R,
    config: &BuildDepsConfig,
    receiver: &E,
) {
    if config.skip_preload {
        return;
    }

    let start = tokio::time::Instant::now();

    let initial_deps = gather_preload_deps(graph, config.legacy_peer_deps);
    if initial_deps.is_empty() {
        return;
    }

    tracing::debug!("Preload phase: {} initial dependencies", initial_deps.len());
    receiver.on_event(BuildEvent::PreloadStart {
        count: initial_deps.len(),
    });

    let preload_config = PreloadConfig {
        legacy_peer_deps: config.legacy_peer_deps,
        concurrency: config.concurrency,
    };

    let stats = preload_manifests(
        initial_deps,
        registry,
        preload_config,
        receiver,
        |_name, _manifest| {
            // Registry client's resolve_package should cache the manifest
        },
    )
    .await;

    tracing::debug!(
        "Preload phase completed: {} success, {} failed",
        stats.success_count,
        stats.failed_count
    );
    receiver.on_event(BuildEvent::PreloadComplete {
        success: stats.success_count,
        failed: stats.failed_count,
    });

    tracing::debug!("Preload phase: {:?}", start.elapsed());
}

/// Run the BFS traversal phase to build the dependency tree.
async fn run_bfs_phase<R: RegistryClient, E: EventReceiver>(
    graph: &mut DependencyGraph,
    registry: &R,
    legacy_peer_deps: bool,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>> {
    let start = tokio::time::Instant::now();

    let mut current_level = vec![graph.root_index];

    while !current_level.is_empty() {
        receiver.on_event(BuildEvent::LevelStart {
            node_count: current_level.len(),
        });
        let mut next_level = Vec::new();

        for node_index in current_level {
            // Add workspace nodes to next level
            for (_, dep) in graph.get_dependency_edges(node_index) {
                if dep.valid
                    && let Some(to) = dep.to
                    && let Some(n) = graph.get_node(to)
                    && n.is_workspace()
                    && node_index == graph.root_index
                {
                    next_level.push(to);
                }
            }

            // Process unresolved dependencies
            let unresolved = collect_unresolved_edges(graph, node_index);
            receiver.on_event(BuildEvent::DependencyCount {
                count: unresolved.len(),
            });

            for edge_info in unresolved {
                match process_dependency(graph, registry, node_index, &edge_info, legacy_peer_deps)
                    .await?
                {
                    ProcessResult::Created(idx) => {
                        receiver.on_event(BuildEvent::Resolved {
                            name: edge_info.name,
                            version: graph
                                .get_node(idx)
                                .map(|n| n.version.clone())
                                .unwrap_or_default(),
                        });
                        next_level.push(idx);
                    }
                    ProcessResult::Reused(idx) => {
                        receiver.on_event(BuildEvent::Reused {
                            name: edge_info.name,
                            version: graph
                                .get_node(idx)
                                .map(|n| n.version.clone())
                                .unwrap_or_default(),
                        });
                    }
                    ProcessResult::Skipped => {
                        receiver.on_event(BuildEvent::Skipped {
                            name: edge_info.name,
                            spec: edge_info.spec,
                        });
                    }
                }
            }
        }

        receiver.on_event(BuildEvent::LevelComplete {
            next_level_count: next_level.len(),
        });
        current_level = next_level;
    }

    tracing::debug!("Build phase: {:?}", start.elapsed());
    Ok(())
}

// ============================================================================
// High-level API
// ============================================================================

use crate::model::package_lock::{LockPackage, PackageLock};
use std::path::Path;

/// Build package-lock.json from a package.json.
///
/// This is the main entry point for dependency resolution. It takes a parsed
/// package.json and returns a complete package-lock.json structure.
///
/// # Arguments
/// * `pkg` - The root package.json
/// * `registry` - Registry client for fetching packages
///
/// # Example
/// ```ignore
/// let pkg: PackageJson = serde_json::from_str(&pkg_content)?;
/// let lock = resolve(&pkg, &registry).await?;
/// ```
pub async fn resolve<R: RegistryClient>(
    pkg: &PackageJson,
    registry: &R,
) -> Result<PackageLock, ResolveError<R::Error>> {
    resolve_with_options(pkg, registry, false, &NoopReceiver).await
}

/// Build package-lock.json with options.
///
/// # Arguments
/// * `pkg` - The root package.json
/// * `registry` - Registry client for fetching packages
/// * `legacy_peer_deps` - If true, skip peer dependencies
/// * `receiver` - Event receiver for progress tracking
pub async fn resolve_with_options<R: RegistryClient, E: EventReceiver>(
    pkg: &PackageJson,
    registry: &R,
    legacy_peer_deps: bool,
    receiver: &E,
) -> Result<PackageLock, ResolveError<R::Error>> {
    // Create graph with root node
    let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg.clone());

    // Add root dependency edges
    let root_index = graph.root_index;
    add_edges_from(&mut graph, root_index, pkg, legacy_peer_deps, true);

    // Build dependency tree
    build_deps_with_receiver(&mut graph, registry, legacy_peer_deps, receiver).await?;

    // Convert to PackageLock
    Ok(graph_to_package_lock(&graph, pkg, Path::new(".")))
}

/// Convert a DependencyGraph to PackageLock.
fn graph_to_package_lock(
    graph: &DependencyGraph,
    pkg: &PackageJson,
    root_path: &Path,
) -> PackageLock {
    let (packages_value, _total) = graph.serialize_to_packages(root_path);

    // Convert Value to HashMap<String, LockPackage>
    let packages: HashMap<String, LockPackage> =
        serde_json::from_value(packages_value).unwrap_or_default();

    PackageLock::new(pkg.name.clone(), pkg.version.clone(), packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::manifest::VersionManifest;
    use crate::traits::registry::mock::MockRegistryClient;

    fn create_version_manifest(name: &str, version: &str) -> VersionManifest {
        VersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        }
    }

    fn create_version_manifest_with_deps(
        name: &str,
        version: &str,
        deps: Vec<(&str, &str)>,
    ) -> VersionManifest {
        let dependencies = deps
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        VersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            dependencies: Some(dependencies),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_build_simple_deps() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "lodash",
            "4.17.21",
            create_version_manifest("lodash", "4.17.21"),
        );

        let root_pkg = PackageJson {
            name: "test-project".to_string(),
            version: "1.0.0".to_string(),
            dependencies: [("lodash".to_string(), "^4.17.0".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());

        // Add initial edges
        let root_index = graph.root_index;
        add_edges_from(&mut graph, root_index, &root_pkg, false, true);

        // Build deps
        build_deps(&mut graph, &registry, false).await.unwrap();

        // Verify
        assert_eq!(graph.graph.node_count(), 2); // root + lodash
        let children = graph.get_physical_children(graph.root_index);
        assert_eq!(children.len(), 1);

        let lodash_node = graph.get_node(children[0]).unwrap();
        assert_eq!(lodash_node.name, "lodash");
        assert_eq!(lodash_node.version, "4.17.21");
    }

    #[tokio::test]
    async fn test_build_transitive_deps() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "express",
            "4.18.0",
            create_version_manifest_with_deps("express", "4.18.0", vec![("debug", "^4.0.0")]),
        );
        registry.add_package("debug", "4.3.0", create_version_manifest("debug", "4.3.0"));

        let root_pkg = PackageJson {
            name: "test-project".to_string(),
            version: "1.0.0".to_string(),
            dependencies: [("express".to_string(), "^4.0.0".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        add_edges_from(&mut graph, root_index, &root_pkg, false, true);

        build_deps(&mut graph, &registry, false).await.unwrap();

        // Should have root + express + debug
        assert_eq!(graph.graph.node_count(), 3);
    }

    #[tokio::test]
    async fn test_resolve_high_level_api() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "lodash",
            "4.17.21",
            create_version_manifest("lodash", "4.17.21"),
        );

        let pkg = PackageJson {
            name: "test-project".to_string(),
            version: "1.0.0".to_string(),
            dependencies: [("lodash".to_string(), "^4.17.0".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let lock = resolve(&pkg, &registry).await.unwrap();

        assert_eq!(lock.name, "test-project");
        assert_eq!(lock.version, "1.0.0");
        assert_eq!(lock.packages.len(), 2); // root + lodash

        // Check lodash is in packages
        let lodash = lock.packages.get("node_modules/lodash").unwrap();
        assert_eq!(lodash.version, Some("4.17.21".to_string()));
    }

    // Helper to create a graph with source -> target for testing update_node_type_from_edge
    // Returns (graph, source_index, target_index) where source is NOT root
    fn create_source_target_graph() -> (DependencyGraph, NodeIndex, NodeIndex) {
        let root_pkg = PackageJson {
            name: "root".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg);

        // Add source node (non-root)
        let source = PackageNode::from_version_manifest(
            "source".to_string(),
            PathBuf::from("node_modules/source"),
            create_version_manifest("source", "1.0.0"),
        );
        let source_index = graph.add_node(source);

        // Add target node
        let target = PackageNode::from_version_manifest(
            "target".to_string(),
            PathBuf::from("node_modules/target"),
            create_version_manifest("target", "1.0.0"),
        );
        let target_index = graph.add_node(target);

        (graph, source_index, target_index)
    }

    #[test]
    fn test_update_node_type_prod_optional_edge() {
        // prod source with optional edge -> target is optional only
        let (mut graph, source_index, target_index) = create_source_target_graph();

        // Mark source as prod
        graph.get_node_mut(source_index).unwrap().is_prod = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Optional);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(!target.is_dev, "should not be dev");
        assert!(target.is_optional, "should be optional");
    }

    #[test]
    fn test_update_node_type_dev_optional_edge() {
        // dev source with optional edge -> target is dev + optional
        let (mut graph, source_index, target_index) = create_source_target_graph();

        // Mark source as dev
        graph.get_node_mut(source_index).unwrap().is_dev = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Optional);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(target.is_dev, "should be dev");
        assert!(target.is_optional, "should be optional");
    }

    #[test]
    fn test_update_node_type_prod_prod_edge() {
        // prod source with prod edge -> target is prod
        let (mut graph, source_index, target_index) = create_source_target_graph();

        graph.get_node_mut(source_index).unwrap().is_prod = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Prod);

        let target = graph.get_node(target_index).unwrap();
        assert!(target.is_prod, "should be prod");
        assert!(!target.is_dev, "should not be dev");
        assert!(!target.is_optional, "should not be optional");
    }

    #[test]
    fn test_update_node_type_dev_prod_edge() {
        // dev source with prod edge -> target is dev only
        let (mut graph, source_index, target_index) = create_source_target_graph();

        graph.get_node_mut(source_index).unwrap().is_dev = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Prod);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(target.is_dev, "should be dev");
        assert!(!target.is_optional, "should not be optional");
    }

    #[test]
    fn test_update_node_type_dev_optional_source_propagates_both() {
        // dev+optional source (devOptional) with prod edge -> target is dev + optional
        let (mut graph, source_index, target_index) = create_source_target_graph();

        // Source is both dev and optional (devOptional package)
        graph.get_node_mut(source_index).unwrap().is_dev = true;
        graph.get_node_mut(source_index).unwrap().is_optional = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Prod);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(target.is_dev, "should be dev (inherited from devOptional source)");
        assert!(
            target.is_optional,
            "should be optional (inherited from devOptional source)"
        );
    }
}
