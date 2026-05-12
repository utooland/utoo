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
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use futures::stream::{FuturesUnordered, StreamExt};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::{HashSet, VecDeque};

#[cfg(feature = "http-tarball")]
use anyhow::Context as _;

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::manifest::NodeManifest;
#[cfg(not(target_arch = "wasm32"))]
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::resolver::preload::{PreloadConfig, extract_transitive_deps, preload_manifests};
use crate::resolver::registry::{ResolveError, resolve_registry_dep};
#[cfg(not(target_arch = "wasm32"))]
use crate::resolver::semver::normalize_spec;
#[cfg(not(target_arch = "wasm32"))]
use crate::resolver::version::resolve_target_version;
use crate::spec::{Catalogs, PackageSpec, Protocol};
use crate::traits::progress::{BuildEvent, EventReceiver, NoopReceiver};
#[cfg(not(target_arch = "wasm32"))]
use crate::traits::registry::RegistryError;
use crate::traits::registry::{RegistryClient, ResolvedPackage};

/// Dispatch a git/github spec to the real `gix`-backed resolver when the
/// `native-git` feature is enabled, otherwise error with a hint.
async fn resolve_git_dep(
    cache_dir: Option<&std::path::Path>,
    spec: &PackageSpec,
    name: &str,
    clone_cache: &GitCloneCache,
) -> anyhow::Result<ResolvedPackage> {
    #[cfg(feature = "native-git")]
    {
        crate::resolver::git::resolve_git_dep(cache_dir, spec, name, clone_cache).await
    }
    #[cfg(not(feature = "native-git"))]
    {
        let _ = (cache_dir, name, clone_cache);
        anyhow::bail!(
            "Git resolution not available for spec '{spec:?}' (enable the 'native-git' feature)"
        )
    }
}

/// Dispatch an HTTP(S) tarball spec to the real resolver when the
/// `http-tarball` feature is enabled, otherwise error with a hint.
///
/// BFS extracts to `<cache_dir>/<name>/_http_<url_hash>/` so install-phase
/// skips re-download; see [`super::http`] module docs.
async fn resolve_http_dep(
    cache_dir: Option<&std::path::Path>,
    url: &str,
    fetch_cache: &HttpFetchCache,
) -> anyhow::Result<ResolvedPackage> {
    #[cfg(feature = "http-tarball")]
    {
        crate::resolver::http::resolve_http_dep(cache_dir, url, fetch_cache).await
    }
    #[cfg(not(feature = "http-tarball"))]
    {
        let _ = (cache_dir, fetch_cache);
        anyhow::bail!(
            "HTTP tarball resolution not available for '{url}' (enable the 'http-tarball' feature)"
        )
    }
}

// Callers construct a `BuildDepsConfig` without touching feature flags — when
// a resolver is disabled the cache alias falls back to `DedupCache<()>`, which
// has the same shape so the struct literal still compiles.
#[cfg(feature = "native-git")]
use crate::resolver::git::GitCloneCache;
#[cfg(feature = "http-tarball")]
use crate::resolver::http::HttpFetchCache;

#[cfg(not(feature = "native-git"))]
type GitCloneCache = crate::resolver::common::DedupCache<()>;
#[cfg(not(feature = "http-tarball"))]
type HttpFetchCache = crate::resolver::common::DedupCache<()>;

// Re-export edge types
pub use super::edges::{
    DependencyEdgeInfo, DependencySource, EdgeContext, add_edges_from, collect_unresolved_edges,
};
pub use crate::model::node::{DevDeps, PeerDeps};

/// Configuration for dependency resolution.
#[derive(Debug, Clone)]
pub struct BuildDepsConfig {
    /// How to handle peer dependencies.
    pub peer_deps: PeerDeps,
    /// Maximum number of concurrent manifest fetches during preload
    pub concurrency: usize,
    /// Whether to skip preload phase (useful when cache is already warm)
    pub skip_preload: bool,
    /// Cache directory for git clones (defaults to `~/.cache/nm`)
    pub cache_dir: Option<PathBuf>,
    /// Shared dedup cache for concurrent git clone operations
    pub git_clone_cache: Arc<GitCloneCache>,
    /// Shared dedup cache for concurrent HTTP tarball fetches
    pub http_fetch_cache: Arc<HttpFetchCache>,
    /// Catalog definitions for the `catalog:` dependency protocol.
    /// Key `""` = default catalog, other keys = named catalogs.
    pub catalogs: Catalogs,
}

impl Default for BuildDepsConfig {
    fn default() -> Self {
        Self {
            peer_deps: PeerDeps::Skip,
            concurrency: crate::resolver::preload::DEFAULT_CONCURRENCY,
            skip_preload: false,
            cache_dir: dirs::home_dir().map(|d| d.join(".cache/nm")),
            git_clone_cache: Arc::new(GitCloneCache::new()),
            http_fetch_cache: Arc::new(HttpFetchCache::new()),
            catalogs: HashMap::new(),
        }
    }
}

impl BuildDepsConfig {
    /// Set how to handle peer dependencies.
    pub fn with_peer_deps(mut self, peer_deps: PeerDeps) -> Self {
        self.peer_deps = peer_deps;
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

    /// Set the cache directory for git clones
    pub fn with_cache_dir(mut self, cache_dir: PathBuf) -> Self {
        self.cache_dir = Some(cache_dir);
        self
    }

    /// Set catalog definitions for `catalog:` protocol resolution.
    pub fn with_catalogs(mut self, catalogs: Catalogs) -> Self {
        self.catalogs = catalogs;
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
///
/// Only registry specs (e.g. `^4.17.0`) are collected. `catalog:` specs are
/// resolved at edge creation time, so by the time this runs they are already
/// concrete registry specs.
fn gather_preload_deps(graph: &DependencyGraph, peer_deps: PeerDeps) -> Vec<(String, String)> {
    use crate::spec::SpecStr;
    use std::collections::HashSet;

    let mut deps = HashSet::new();

    let collect = |node_index: NodeIndex, deps: &mut HashSet<(String, String)>| {
        for (_, edge) in graph.get_dependency_edges(node_index) {
            if edge.valid {
                continue;
            }
            if peer_deps == PeerDeps::Skip && edge.edge_type == EdgeType::Peer {
                continue;
            }
            if edge.spec.is_registry_spec() {
                deps.insert((edge.name.clone(), edge.spec.clone()));
            }
        }
    };

    collect(graph.root_index, &mut deps);

    for node_index in graph.graph.node_indices() {
        if let Some(node) = graph.get_node(node_index)
            && node.is_workspace()
        {
            collect(node_index, &mut deps);
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

/// Handle a `file:` dep: dir → Link node inline (returns
/// `ControlFlow::Break`); tarball → stream bytes through the shared
/// `commit_tarball_bytes` and hand the `ResolvedPackage` back to the
/// normal BFS flow via `ControlFlow::Continue`.
#[cfg(feature = "http-tarball")]
async fn process_file_dep<E>(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    conflict_parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    path_spec: &str,
    cache_dir: Option<&Path>,
) -> Result<std::ops::ControlFlow<ProcessResult, ResolvedPackage>, ResolveError<E>> {
    use std::ops::ControlFlow;

    use crate::resolver::http::file_cache_slot;
    use crate::resolver::tar::commit_tarball_bytes;

    let file_err = |source: anyhow::Error| ResolveError::File {
        spec: edge.spec.clone(),
        source,
    };

    // Base dir is the on-disk source for root/workspace/link nodes, or
    // the parent of the `file:<abs>` tarball URL stamped on a transitive
    // file-tarball dep's manifest. Registry nodes have no valid base.
    let node = graph.get_node(node_index);
    let base = node
        .filter(|n| n.is_root() || n.is_workspace() || n.is_link())
        .map(|n| n.path.clone())
        .or_else(|| {
            let NodeManifest::Registry(m) = &node?.manifest else {
                return None;
            };
            let url = m.dist.tarball.as_deref()?.strip_prefix("file:")?;
            std::path::Path::new(url).parent().map(Path::to_path_buf)
        })
        .ok_or_else(|| ResolveError::Unsupported {
            spec: edge.spec.clone(),
            reason: "transitive file: deps inside a published registry package are not supported",
        })?;
    let abs = base.join(path_spec);

    let meta = match std::fs::metadata(&abs) {
        Ok(m) => m,
        Err(_) if edge.edge_type == EdgeType::Optional => {
            return Ok(ControlFlow::Break(ProcessResult::Skipped));
        }
        Err(e) => {
            return Err(file_err(
                anyhow::Error::new(e).context(format!("file: target {}", abs.display())),
            ));
        }
    };

    if meta.is_dir() {
        // Symlink install — same graph shape as a workspace link. We
        // intentionally do not walk the linked package's transitive deps
        // (npm-link semantics: the linked dir owns its own node_modules).
        let pkg = crate::model::util::read_package_json(&abs)
            .await
            .map_err(file_err)?;
        let idx = graph.add_node(PackageNode::link_from_package_json(abs, pkg));
        graph.add_physical_edge(conflict_parent, idx);
        graph.mark_dependency_resolved(edge.edge_id, idx);
        update_node_type_from_edge(graph, node_index, idx, &edge.edge_type);
        return Ok(ControlFlow::Break(ProcessResult::Created(idx)));
    }

    let cache_dir = cache_dir
        .ok_or_else(|| file_err(anyhow::anyhow!("cache_dir required for file: tarball")))?
        .to_path_buf();
    let slot = file_cache_slot(&abs);
    let pinned = format!("file:{}", abs.display());
    let manifest = match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let bytes = std::fs::read(&abs)
            .with_context(|| format!("failed to read tarball {}", abs.display()))?;
        commit_tarball_bytes(&cache_dir, &bytes, pinned, &slot)
    })
    .await
    {
        Ok(Ok(m)) => m,
        Ok(Err(_)) | Err(_) if edge.edge_type == EdgeType::Optional => {
            return Ok(ControlFlow::Break(ProcessResult::Skipped));
        }
        Ok(Err(source)) => return Err(file_err(source)),
        Err(join) => return Err(file_err(join.into())),
    };
    Ok(ControlFlow::Continue(ResolvedPackage {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        manifest: Arc::new(manifest),
    }))
}

/// Process a single dependency edge.
///
/// This is the core logic for resolving a dependency:
/// 1. Check if an existing compatible version can be reused
/// 2. If not, resolve from registry (or git) and create a new node
/// 3. Handle conflicts by installing nested
///
/// Non-registry specs (git/github, http tarball) are routed through
/// [`resolve_git_dep`] / [`resolve_http_dep`] instead of the registry.
///
/// # Arguments
/// * `graph` - The dependency graph
/// * `registry` - Registry client for fetching packages
/// * `node_index` - The node that has this dependency
/// * `edge_info` - Information about the dependency edge
/// * `config` - Build configuration (peer_deps, cache_dir, etc.)
///
/// # Returns
/// The result of processing (reused, created, or skipped)
pub async fn process_dependency<R: RegistryClient>(
    graph: &mut DependencyGraph,
    registry: &R,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    config: &BuildDepsConfig,
) -> Result<ProcessResult, ResolveError<R::Error>> {
    // Find installation location
    match graph.find_compatible_node(node_index, &edge_info.name, &edge_info.spec) {
        FindResult::Reuse(existing_index) => {
            // Mark edge as resolved
            graph.mark_dependency_resolved(edge_info.edge_id, existing_index);

            // Update target node type
            update_node_type_from_edge(graph, node_index, existing_index, &edge_info.edge_type);

            Ok(ProcessResult::Reused(existing_index))
        }
        FindResult::Conflict(conflict_parent) | FindResult::New(conflict_parent) => {
            // Parse spec once and exhaustively route by variant.
            // The exhaustive match ensures the compiler forces a decision for any
            // new PackageSpec variant — no silent fall-through to the wrong resolver.
            let parsed_spec = PackageSpec::from(edge_info.spec.as_str());
            let resolved: ResolvedPackage = match &parsed_spec {
                PackageSpec::Git { .. } | PackageSpec::GitHub { .. } => {
                    // TODO: add spec => version expiry check so stale git caches
                    // are invalidated (e.g. branch refs that have moved forward).
                    match resolve_git_dep(
                        config.cache_dir.as_deref(),
                        &parsed_spec,
                        &edge_info.name,
                        &config.git_clone_cache,
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) if edge_info.edge_type == EdgeType::Optional => {
                            tracing::debug!(
                                "Skipped optional non-registry dependency {}@{}",
                                edge_info.name,
                                edge_info.spec
                            );
                            return Ok(ProcessResult::Skipped);
                        }
                        Err(e) => {
                            return Err(ResolveError::Git {
                                url: edge_info.spec.clone(),
                                source: e,
                            });
                        }
                    }
                }
                PackageSpec::Local {
                    protocol: Protocol::Workspace,
                    ..
                } => {
                    // workspace: deps are resolved during graph initialisation.
                    // If we reach here the workspace node wasn't found — skip
                    // silently rather than aborting the whole resolution.
                    tracing::debug!(
                        "Skipping unresolved workspace dependency {}@{}",
                        edge_info.name,
                        edge_info.spec
                    );
                    return Ok(ProcessResult::Skipped);
                }
                PackageSpec::Local {
                    protocol: Protocol::File,
                    path,
                } => {
                    #[cfg(feature = "http-tarball")]
                    {
                        match process_file_dep(
                            graph,
                            node_index,
                            conflict_parent,
                            edge_info,
                            path,
                            config.cache_dir.as_deref(),
                        )
                        .await?
                        {
                            std::ops::ControlFlow::Break(r) => return Ok(r),
                            std::ops::ControlFlow::Continue(pkg) => pkg,
                        }
                    }
                    #[cfg(not(feature = "http-tarball"))]
                    {
                        let _ = path;
                        return Err(ResolveError::Unsupported {
                            spec: edge_info.spec.clone(),
                            reason: "file: deps require the 'http-tarball' feature",
                        });
                    }
                }
                PackageSpec::Local { .. } => {
                    return Err(ResolveError::Unsupported {
                        spec: edge_info.spec.clone(),
                        reason: "local (link:/portal:) dependencies are not yet supported",
                    });
                }
                PackageSpec::Http { url } => {
                    match resolve_http_dep(
                        config.cache_dir.as_deref(),
                        url,
                        &config.http_fetch_cache,
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) if edge_info.edge_type == EdgeType::Optional => {
                            tracing::debug!(
                                "Skipped optional HTTP dependency {}@{}",
                                edge_info.name,
                                edge_info.spec
                            );
                            return Ok(ProcessResult::Skipped);
                        }
                        Err(e) => {
                            return Err(ResolveError::Http {
                                url: url.clone(),
                                source: e,
                            });
                        }
                    }
                }
                PackageSpec::Registry { .. } => {
                    match resolve_registry_dep(
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
                    }
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
                match resolve_registry_dep(
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
                &*resolved.manifest,
                &EdgeContext::new(config.peer_deps, DevDeps::Exclude),
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
/// * `peer_deps` - How to handle peer dependencies
///
/// # Example
/// ```ignore
/// let mut graph = DependencyGraph::new(path, package_json);
/// // Add initial dependency edges to root...
/// build_deps(&mut graph, &registry, PeerDeps::Include).await?;
/// ```
pub async fn build_deps<R: RegistryClient>(
    graph: &mut DependencyGraph,
    registry: &R,
    peer_deps: PeerDeps,
) -> Result<(), ResolveError<R::Error>> {
    let config = BuildDepsConfig::default().with_peer_deps(peer_deps);
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
/// * `peer_deps` - How to handle peer dependencies
/// * `receiver` - Event receiver for handling build events
pub async fn build_deps_with_receiver<R: RegistryClient, E: EventReceiver>(
    graph: &mut DependencyGraph,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>> {
    let config = BuildDepsConfig::default().with_peer_deps(peer_deps);
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
/// * `config` - Build configuration (concurrency, peer_deps, skip_preload)
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
        "Starting dependency tree build, peer_deps: {:?}, concurrency: {}, skip_preload: {}",
        config.peer_deps,
        config.concurrency,
        config.skip_preload
    );

    #[cfg(not(target_arch = "wasm32"))]
    if !config.skip_preload && !registry.registry_url().is_empty() {
        run_main_loop_bfs(graph, registry, &config, receiver).await?;
    } else {
        // Keep the existing path for warm project-cache runs and generic
        // RegistryClient implementations that do not expose a raw registry URL.
        run_preload_phase(graph, registry, &config, receiver).await;
        run_bfs_phase(graph, registry, &config, receiver).await?;
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_preload_phase(graph, registry, &config, receiver).await;
        run_bfs_phase(graph, registry, &config, receiver).await?;
    }

    receiver.on_event(BuildEvent::Complete {
        total_nodes: graph.graph.node_count(),
    });

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
type WaitingEdge = (NodeIndex, DependencyEdgeInfo);

#[cfg(not(target_arch = "wasm32"))]
enum FetchRequest {
    Full { name: String },
    Version { name: String, spec: String },
}

#[cfg(not(target_arch = "wasm32"))]
enum FetchDone {
    Full {
        name: String,
        result: anyhow::Result<(Vec<u8>, Option<String>)>,
    },
    Version {
        name: String,
        spec: String,
        result: anyhow::Result<Vec<u8>>,
    },
}

#[cfg(not(target_arch = "wasm32"))]
type FetchFuture = tokio::task::JoinHandle<FetchDone>;

#[cfg(not(target_arch = "wasm32"))]
fn registry_error<E>(message: impl Into<String>) -> ResolveError<E>
where
    E: From<RegistryError>,
{
    ResolveError::Registry(RegistryError(anyhow::anyhow!(message.into())).into())
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_full_manifest_inline(raw_bytes: Vec<u8>) -> anyhow::Result<Arc<FullManifest>> {
    let mut parse_buf = raw_bytes.clone();
    let mut manifest: FullManifest = simd_json::serde::from_slice(&mut parse_buf)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {e}"))?;
    manifest.raw = Arc::from(raw_bytes);
    Ok(Arc::new(manifest))
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_core_manifest_inline(mut bytes: Vec<u8>) -> anyhow::Result<Arc<CoreVersionManifest>> {
    simd_json::serde::from_slice::<CoreVersionManifest>(&mut bytes)
        .map(Arc::new)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {e}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_registry_manifest(registry_url: String, request: FetchRequest) -> FetchFuture {
    use crate::service::{
        FetchManifestBytesResult, FetchManifestOptions, FetchVersionManifestOptions,
        MetadataFormat, fetch_full_manifest_bytes, fetch_version_manifest_bytes,
    };

    tokio::spawn(async move {
        match request {
            FetchRequest::Full { name } => {
                let result = fetch_full_manifest_bytes(FetchManifestOptions {
                    registry_url: &registry_url,
                    name: &name,
                    format: MetadataFormat::Abbreviated,
                    etag: None,
                })
                .await
                .and_then(|result| match result {
                    FetchManifestBytesResult::Ok(bytes, etag) => Ok((bytes, etag)),
                    FetchManifestBytesResult::NotModified => {
                        Err(anyhow::anyhow!("304 Not Modified without etag context"))
                    }
                });
                FetchDone::Full { name, result }
            }
            FetchRequest::Version { name, spec } => {
                let result = fetch_version_manifest_bytes(FetchVersionManifestOptions {
                    registry_url: &registry_url,
                    name: &name,
                    spec: &spec,
                    format: MetadataFormat::Abbreviated,
                })
                .await;
                FetchDone::Version { name, spec, result }
            }
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn pump_fetches(
    fetches: &mut FuturesUnordered<FetchFuture>,
    queue: &mut VecDeque<FetchRequest>,
    registry_url: &str,
    concurrency: usize,
) {
    while fetches.len() < concurrency {
        let Some(request) = queue.pop_front() else {
            break;
        };
        fetches.push(fetch_registry_manifest(registry_url.to_string(), request));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn schedule_registry_fetch(
    name: String,
    spec: String,
    supports_semver: bool,
    full_cache: &HashMap<String, Arc<FullManifest>>,
    version_cache: &HashMap<(String, String), Arc<CoreVersionManifest>>,
    full_failures: &HashMap<String, String>,
    version_failures: &HashMap<(String, String), String>,
    inflight_full: &mut HashSet<String>,
    inflight_version: &mut HashSet<(String, String)>,
    fetch_queue: &mut VecDeque<FetchRequest>,
) {
    let (real_name, real_spec) = normalize_spec(&name, &spec);
    if supports_semver {
        let key = (real_name, real_spec);
        if version_cache.contains_key(&key)
            || version_failures.contains_key(&key)
            || !inflight_version.insert(key.clone())
        {
            return;
        }
        fetch_queue.push_back(FetchRequest::Version {
            name: key.0,
            spec: key.1,
        });
    } else if !full_cache.contains_key(&real_name)
        && !full_failures.contains_key(&real_name)
        && inflight_full.insert(real_name.clone())
    {
        fetch_queue.push_back(FetchRequest::Full { name: real_name });
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn schedule_transitive_prefetches(
    manifest: &CoreVersionManifest,
    preload_config: &PreloadConfig,
    supports_semver: bool,
    full_cache: &HashMap<String, Arc<FullManifest>>,
    version_cache: &HashMap<(String, String), Arc<CoreVersionManifest>>,
    full_failures: &HashMap<String, String>,
    version_failures: &HashMap<(String, String), String>,
    inflight_full: &mut HashSet<String>,
    inflight_version: &mut HashSet<(String, String)>,
    fetch_queue: &mut VecDeque<FetchRequest>,
) {
    for (name, spec) in extract_transitive_deps(manifest, preload_config) {
        schedule_registry_fetch(
            name,
            spec,
            supports_semver,
            full_cache,
            version_cache,
            full_failures,
            version_failures,
            inflight_full,
            inflight_version,
            fetch_queue,
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn try_reuse_dependency(
    graph: &mut DependencyGraph,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
) -> Option<ProcessResult> {
    match graph.find_compatible_node(parent, &edge.name, &edge.spec) {
        FindResult::Reuse(existing_index) => {
            graph.mark_dependency_resolved(edge.edge_id, existing_index);
            update_node_type_from_edge(graph, parent, existing_index, &edge.edge_type);
            Some(ProcessResult::Reused(existing_index))
        }
        FindResult::Conflict(_) | FindResult::New(_) => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn process_dependency_with_resolved(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    resolved: &ResolvedPackage,
    config: &BuildDepsConfig,
) -> ProcessResult {
    match graph.find_compatible_node(node_index, &edge_info.name, &edge_info.spec) {
        FindResult::Reuse(existing_index) => {
            graph.mark_dependency_resolved(edge_info.edge_id, existing_index);
            update_node_type_from_edge(graph, node_index, existing_index, &edge_info.edge_type);
            ProcessResult::Reused(existing_index)
        }
        FindResult::Conflict(conflict_parent) | FindResult::New(conflict_parent) => {
            let new_node = create_package_node(&edge_info.name, resolved, conflict_parent, graph);
            let new_index = graph.add_node(new_node);
            graph.add_physical_edge(conflict_parent, new_index);
            graph.mark_dependency_resolved(edge_info.edge_id, new_index);
            update_node_type_from_edge(graph, node_index, new_index, &edge_info.edge_type);
            add_edges_from(
                graph,
                new_index,
                &*resolved.manifest,
                &EdgeContext::new(config.peer_deps, DevDeps::Exclude),
            );
            ProcessResult::Created(new_index)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn chain_err<E>(
    graph: &DependencyGraph,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    inner: ResolveError<E>,
) -> ResolveError<E> {
    let mut chain = graph.logical_ancestry(parent);
    chain.push((edge.name.clone(), edge.spec.clone()));
    ResolveError::WithChain {
        chain,
        source: Box::new(inner),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle_processed<E: EventReceiver>(
    graph: &DependencyGraph,
    receiver: &E,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    processed: &ProcessResult,
    next_level: &mut Vec<NodeIndex>,
) {
    match processed {
        ProcessResult::Created(idx) => {
            if let Some(node) = graph.get_node(*idx) {
                receiver.on_event(BuildEvent::Resolved {
                    name: &edge.name,
                    version: &node.version,
                });
                if let NodeManifest::Registry(ref manifest) = node.manifest {
                    let parent_path = graph.get_node(parent).map(|p| p.path.as_path());
                    receiver.on_event(BuildEvent::PackagePlaced {
                        package: manifest.as_ref().into(),
                        path: &node.path,
                        parent_path,
                    });
                }
            }
            next_level.push(*idx);
        }
        ProcessResult::Reused(idx) => {
            if let Some(node) = graph.get_node(*idx) {
                receiver.on_event(BuildEvent::Reused {
                    name: &edge.name,
                    version: &node.version,
                });
            }
        }
        ProcessResult::Skipped => {
            receiver.on_event(BuildEvent::Skipped {
                name: &edge.name,
                spec: &edge.spec,
            });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_from_full_manifest<RE>(
    edge: &DependencyEdgeInfo,
    full: &FullManifest,
    real_spec: &str,
    core_cache: &mut HashMap<(String, String), Arc<CoreVersionManifest>>,
) -> Result<Option<ResolvedPackage>, ResolveError<RE>> {
    if full.versions.is_empty() {
        if edge.edge_type == EdgeType::Optional {
            return Ok(None);
        }
        return Err(ResolveError::NoVersions(full.name.clone()));
    }

    let version = match resolve_target_version(full.into(), real_spec) {
        Ok(version) => version,
        Err(_) if edge.edge_type == EdgeType::Optional => return Ok(None),
        Err(e) => {
            return Err(ResolveError::Version(format!(
                "{}@{}: {}",
                edge.name, real_spec, e
            )));
        }
    };

    let cache_key = (full.name.clone(), version.clone());
    let manifest = match core_cache.get(&cache_key).cloned() {
        Some(manifest) => manifest,
        None => {
            let Some(manifest) = full.get_core_version(&version).map(Arc::new) else {
                if edge.edge_type == EdgeType::Optional {
                    return Ok(None);
                }
                return Err(ResolveError::ManifestNotFound {
                    name: edge.name.clone(),
                    version,
                });
            };
            core_cache.insert(cache_key, Arc::clone(&manifest));
            manifest
        }
    };

    Ok(Some(ResolvedPackage {
        name: edge.name.clone(),
        version: manifest.version.clone(),
        manifest,
    }))
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn apply_fetch_result(
    done: FetchDone,
    full_cache: &mut HashMap<String, Arc<FullManifest>>,
    version_cache: &mut HashMap<(String, String), Arc<CoreVersionManifest>>,
    full_waiters: &mut HashMap<String, Vec<WaitingEdge>>,
    version_waiters: &mut HashMap<(String, String), Vec<WaitingEdge>>,
    full_failures: &mut HashMap<String, String>,
    version_failures: &mut HashMap<(String, String), String>,
    inflight_full: &mut HashSet<String>,
    inflight_version: &mut HashSet<(String, String)>,
    fetch_queue: &mut VecDeque<FetchRequest>,
    preload_config: &PreloadConfig,
    supports_semver: bool,
    level_pending: &mut VecDeque<WaitingEdge>,
) {
    match done {
        FetchDone::Full { name, result } => {
            inflight_full.remove(&name);
            match result.and_then(|(bytes, _etag)| parse_full_manifest_inline(bytes)) {
                Ok(full) => {
                    full_cache.insert(name.clone(), full);
                }
                Err(e) => {
                    full_failures.insert(name.clone(), format!("{e:#}"));
                }
            }
            if let Some(waiters) = full_waiters.remove(&name) {
                level_pending.extend(waiters);
            }
        }
        FetchDone::Version { name, spec, result } => {
            let key = (name, spec);
            inflight_version.remove(&key);
            match result.and_then(parse_core_manifest_inline) {
                Ok(manifest) => {
                    version_cache.insert(key.clone(), Arc::clone(&manifest));
                    schedule_transitive_prefetches(
                        &manifest,
                        preload_config,
                        supports_semver,
                        full_cache,
                        version_cache,
                        full_failures,
                        version_failures,
                        inflight_full,
                        inflight_version,
                        fetch_queue,
                    );
                }
                Err(e) => {
                    version_failures.insert(key.clone(), format!("{e:#}"));
                }
            }
            if let Some(waiters) = version_waiters.remove(&key) {
                level_pending.extend(waiters);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn run_main_loop_bfs<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: &BuildDepsConfig,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: RegistryClient,
    E: EventReceiver,
{
    use crate::spec::SpecStr;

    let registry_url = registry.registry_url().trim_end_matches('/').to_string();
    let supports_semver = registry.supports_semver_resolution();
    let concurrency = config.concurrency.max(1);
    let preload_config = PreloadConfig {
        peer_deps: config.peer_deps,
        concurrency,
    };

    let mut full_cache: HashMap<String, Arc<FullManifest>> = HashMap::new();
    let mut version_cache: HashMap<(String, String), Arc<CoreVersionManifest>> = HashMap::new();
    let mut core_cache: HashMap<(String, String), Arc<CoreVersionManifest>> = HashMap::new();
    let mut full_waiters: HashMap<String, Vec<WaitingEdge>> = HashMap::new();
    let mut version_waiters: HashMap<(String, String), Vec<WaitingEdge>> = HashMap::new();
    let mut full_failures: HashMap<String, String> = HashMap::new();
    let mut version_failures: HashMap<(String, String), String> = HashMap::new();
    let mut inflight_full: HashSet<String> = HashSet::new();
    let mut inflight_version: HashSet<(String, String)> = HashSet::new();
    let mut fetch_queue: VecDeque<FetchRequest> = VecDeque::new();
    let mut fetches: FuturesUnordered<FetchFuture> = FuturesUnordered::new();

    let root_idx = graph.root_index;
    let mut current_level = vec![root_idx];

    while !current_level.is_empty() {
        receiver.on_event(BuildEvent::LevelStart {
            node_count: current_level.len(),
        });

        let mut next_level = Vec::new();
        let mut level_pending = VecDeque::new();

        for node_index in &current_level {
            for (_, dep) in graph.get_dependency_edges(*node_index) {
                if dep.valid
                    && let Some(to) = dep.to
                    && let Some(n) = graph.get_node(to)
                    && n.is_workspace()
                    && *node_index == root_idx
                {
                    next_level.push(to);
                }
            }

            let unresolved = collect_unresolved_edges(graph, *node_index);
            receiver.on_event(BuildEvent::DependencyCount {
                count: unresolved.len(),
            });
            for edge in unresolved {
                level_pending.push_back((*node_index, edge));
            }
        }

        loop {
            pump_fetches(&mut fetches, &mut fetch_queue, &registry_url, concurrency);

            while let Some((parent, edge)) = level_pending.pop_front() {
                receiver.on_event(BuildEvent::Resolving { name: &edge.name });

                if !edge.spec.is_registry_spec() {
                    let processed = process_dependency(graph, registry, parent, &edge, config)
                        .await
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
                    handle_processed(graph, receiver, parent, &edge, &processed, &mut next_level);
                    continue;
                }

                if let Some(processed) = try_reuse_dependency(graph, parent, &edge) {
                    handle_processed(graph, receiver, parent, &edge, &processed, &mut next_level);
                    continue;
                }

                let (real_name, real_spec) = normalize_spec(&edge.name, &edge.spec);
                if supports_semver {
                    let key = (real_name.clone(), real_spec.clone());
                    if let Some(error) = version_failures.get(&key) {
                        if edge.edge_type == EdgeType::Optional {
                            receiver.on_event(BuildEvent::Skipped {
                                name: &edge.name,
                                spec: &edge.spec,
                            });
                            continue;
                        }
                        return Err(chain_err(
                            graph,
                            parent,
                            &edge,
                            registry_error(format!("{}@{}: {error}", real_name, real_spec)),
                        ));
                    }

                    if let Some(manifest) = version_cache.get(&key).cloned() {
                        let resolved = ResolvedPackage {
                            name: edge.name.clone(),
                            version: manifest.version.clone(),
                            manifest,
                        };
                        let processed = if graph
                            .check_override(parent, &edge.name, Some(&resolved.version))
                            .is_some()
                        {
                            process_dependency(graph, registry, parent, &edge, config)
                                .await
                                .map_err(|inner| chain_err(graph, parent, &edge, inner))?
                        } else {
                            receiver.on_event(BuildEvent::PackageResolved(
                                (&*resolved.manifest).into(),
                            ));
                            schedule_transitive_prefetches(
                                &resolved.manifest,
                                &preload_config,
                                supports_semver,
                                &full_cache,
                                &version_cache,
                                &full_failures,
                                &version_failures,
                                &mut inflight_full,
                                &mut inflight_version,
                                &mut fetch_queue,
                            );
                            process_dependency_with_resolved(
                                graph, parent, &edge, &resolved, config,
                            )
                        };
                        handle_processed(
                            graph,
                            receiver,
                            parent,
                            &edge,
                            &processed,
                            &mut next_level,
                        );
                        continue;
                    }

                    let waiters = version_waiters.entry(key.clone()).or_default();
                    waiters.push((parent, edge));
                    if inflight_version.insert(key.clone()) {
                        fetch_queue.push_back(FetchRequest::Version {
                            name: key.0,
                            spec: key.1,
                        });
                    }
                } else {
                    if let Some(error) = full_failures.get(&real_name) {
                        if edge.edge_type == EdgeType::Optional {
                            receiver.on_event(BuildEvent::Skipped {
                                name: &edge.name,
                                spec: &edge.spec,
                            });
                            continue;
                        }
                        return Err(chain_err(
                            graph,
                            parent,
                            &edge,
                            registry_error(format!("{}: {error}", real_name)),
                        ));
                    }

                    if let Some(full) = full_cache.get(&real_name).cloned() {
                        let Some(resolved) = resolve_from_full_manifest::<R::Error>(
                            &edge,
                            &full,
                            &real_spec,
                            &mut core_cache,
                        )
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?
                        else {
                            receiver.on_event(BuildEvent::Skipped {
                                name: &edge.name,
                                spec: &edge.spec,
                            });
                            continue;
                        };

                        let processed = if graph
                            .check_override(parent, &edge.name, Some(&resolved.version))
                            .is_some()
                        {
                            process_dependency(graph, registry, parent, &edge, config)
                                .await
                                .map_err(|inner| chain_err(graph, parent, &edge, inner))?
                        } else {
                            receiver.on_event(BuildEvent::PackageResolved(
                                (&*resolved.manifest).into(),
                            ));
                            schedule_transitive_prefetches(
                                &resolved.manifest,
                                &preload_config,
                                supports_semver,
                                &full_cache,
                                &version_cache,
                                &full_failures,
                                &version_failures,
                                &mut inflight_full,
                                &mut inflight_version,
                                &mut fetch_queue,
                            );
                            process_dependency_with_resolved(
                                graph, parent, &edge, &resolved, config,
                            )
                        };
                        handle_processed(
                            graph,
                            receiver,
                            parent,
                            &edge,
                            &processed,
                            &mut next_level,
                        );
                        continue;
                    }

                    let waiters = full_waiters.entry(real_name.clone()).or_default();
                    waiters.push((parent, edge));
                    if inflight_full.insert(real_name.clone()) {
                        fetch_queue.push_back(FetchRequest::Full { name: real_name });
                    }
                }

                pump_fetches(&mut fetches, &mut fetch_queue, &registry_url, concurrency);
            }

            if full_waiters.is_empty() && version_waiters.is_empty() {
                break;
            }

            let Some(done) = fetches.next().await else {
                let mut fallback = Vec::new();
                for (_, waiters) in full_waiters.drain() {
                    fallback.extend(waiters);
                }
                for (_, waiters) in version_waiters.drain() {
                    fallback.extend(waiters);
                }
                for (parent, edge) in fallback {
                    let processed = process_dependency(graph, registry, parent, &edge, config)
                        .await
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
                    handle_processed(graph, receiver, parent, &edge, &processed, &mut next_level);
                }
                break;
            };
            let done = done.map_err(|e| {
                registry_error::<R::Error>(format!("manifest fetch task failed: {e}"))
            })?;

            apply_fetch_result(
                done,
                &mut full_cache,
                &mut version_cache,
                &mut full_waiters,
                &mut version_waiters,
                &mut full_failures,
                &mut version_failures,
                &mut inflight_full,
                &mut inflight_version,
                &mut fetch_queue,
                &preload_config,
                supports_semver,
                &mut level_pending,
            );
        }

        receiver.on_event(BuildEvent::LevelComplete {
            next_level_count: next_level.len(),
        });
        current_level = next_level;
    }

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

    let initial_deps = gather_preload_deps(graph, config.peer_deps);
    if initial_deps.is_empty() {
        return;
    }

    tracing::debug!("Preload phase: {} initial dependencies", initial_deps.len());
    receiver.on_event(BuildEvent::PreloadStart {
        count: initial_deps.len(),
    });

    let preload_config = PreloadConfig {
        peer_deps: config.peer_deps,
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
    config: &BuildDepsConfig,
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
                receiver.on_event(BuildEvent::Resolving {
                    name: &edge_info.name,
                });
                let result = process_dependency(graph, registry, node_index, &edge_info, config)
                    .await
                    .map_err(|inner| {
                        let mut chain = graph.logical_ancestry(node_index);
                        chain.push((edge_info.name.clone(), edge_info.spec.clone()));
                        ResolveError::WithChain {
                            chain,
                            source: Box::new(inner),
                        }
                    });
                match result? {
                    ProcessResult::Created(idx) => {
                        // Extract node info for events
                        if let Some(node) = graph.get_node(idx) {
                            receiver.on_event(BuildEvent::Resolved {
                                name: &edge_info.name,
                                version: &node.version,
                            });

                            // Send PackagePlaced for pipeline cloning
                            if let NodeManifest::Registry(ref manifest) = node.manifest {
                                // Get parent path for dependency ordering
                                let parent_path = graph
                                    .get_node(node_index)
                                    .map(|parent| parent.path.as_path());
                                receiver.on_event(BuildEvent::PackagePlaced {
                                    package: manifest.as_ref().into(),
                                    path: &node.path,
                                    parent_path,
                                });
                            }
                        }

                        next_level.push(idx);
                    }
                    ProcessResult::Reused(idx) => {
                        if let Some(node) = graph.get_node(idx) {
                            receiver.on_event(BuildEvent::Reused {
                                name: &edge_info.name,
                                version: &node.version,
                            });
                        }
                    }
                    ProcessResult::Skipped => {
                        receiver.on_event(BuildEvent::Skipped {
                            name: &edge_info.name,
                            spec: &edge_info.spec,
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

use crate::model::package_lock::PackageLock;
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
    resolve_with_options(pkg, registry, PeerDeps::Include, &NoopReceiver).await
}

/// Build package-lock.json with options.
///
/// # Arguments
/// * `pkg` - The root package.json
/// * `registry` - Registry client for fetching packages
/// * `peer_deps` - How to handle peer dependencies
/// * `receiver` - Event receiver for progress tracking
pub async fn resolve_with_options<R: RegistryClient, E: EventReceiver>(
    pkg: &PackageJson,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<PackageLock, ResolveError<R::Error>> {
    // Create graph with root node
    let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg.clone());

    // Add root dependency edges
    let root_index = graph.root_index;
    add_edges_from(
        &mut graph,
        root_index,
        pkg,
        &EdgeContext::new(peer_deps, DevDeps::Include),
    );

    // Build dependency tree
    build_deps_with_receiver(&mut graph, registry, peer_deps, receiver).await?;

    // Convert to PackageLock
    Ok(graph_to_package_lock(&graph, pkg, Path::new(".")))
}

/// Convert a DependencyGraph to PackageLock.
fn graph_to_package_lock(
    graph: &DependencyGraph,
    pkg: &PackageJson,
    root_path: &Path,
) -> PackageLock {
    let (packages, _total) = graph.serialize_to_packages(root_path);
    PackageLock::new(&pkg.name, &pkg.version, packages)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::model::manifest::CoreVersionManifest;
    use crate::traits::registry::mock::MockRegistryClient;

    fn create_version_manifest(name: &str, version: &str) -> CoreVersionManifest {
        CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        }
    }

    fn create_version_manifest_with_deps(
        name: &str,
        version: &str,
        deps: Vec<(&str, &str)>,
    ) -> CoreVersionManifest {
        let dependencies = deps
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        CoreVersionManifest {
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

        let root_pkg = PackageJson::new("test-project", "1.0.0");
        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([(
                "lodash".to_string(),
                "^4.17.0".to_string(),
            )])),
            ..root_pkg
        };

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());

        // Add initial edges
        let root_index = graph.root_index;
        add_edges_from(
            &mut graph,
            root_index,
            &root_pkg,
            &EdgeContext::new(PeerDeps::Include, DevDeps::Include),
        );

        // Build deps
        build_deps(&mut graph, &registry, PeerDeps::Include)
            .await
            .unwrap();

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

        let root_pkg = PackageJson::new("test-project", "1.0.0");
        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([(
                "express".to_string(),
                "^4.0.0".to_string(),
            )])),
            ..root_pkg
        };

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        add_edges_from(
            &mut graph,
            root_index,
            &root_pkg,
            &EdgeContext::new(PeerDeps::Include, DevDeps::Include),
        );

        build_deps(&mut graph, &registry, PeerDeps::Include)
            .await
            .unwrap();

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

        let pkg = PackageJson::new("test-project", "1.0.0");
        let pkg = PackageJson {
            dependencies: Some(HashMap::from([(
                "lodash".to_string(),
                "^4.17.0".to_string(),
            )])),
            ..pkg
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
        let root_pkg = PackageJson::new("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg);

        // Add source node (non-root)
        let source = PackageNode::from_version_manifest(
            "source".to_string(),
            PathBuf::from("node_modules/source"),
            Arc::new(create_version_manifest("source", "1.0.0")),
        );
        let source_index = graph.add_node(source);

        // Add target node
        let target = PackageNode::from_version_manifest(
            "target".to_string(),
            PathBuf::from("node_modules/target"),
            Arc::new(create_version_manifest("target", "1.0.0")),
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
        assert!(
            target.is_dev,
            "should be dev (inherited from devOptional source)"
        );
        assert!(
            target.is_optional,
            "should be optional (inherited from devOptional source)"
        );
    }

    #[test]
    fn test_edge_context_resolves_catalog_at_creation() {
        // catalog: specs should be resolved to concrete versions at edge creation
        let root_pkg = PackageJson::new("root", "1.0.0");
        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([
                ("lodash".to_string(), "^4.17.0".to_string()),
                ("react".to_string(), "catalog:".to_string()),
                ("tslib".to_string(), "catalog:legacy".to_string()),
            ])),
            ..root_pkg
        };

        let catalogs: Catalogs = HashMap::from([
            (
                "".to_string(),
                HashMap::from([("react".to_string(), "^18.0.0".to_string())]),
            ),
            (
                "legacy".to_string(),
                HashMap::from([("tslib".to_string(), "^2.0.0".to_string())]),
            ),
        ]);

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        let ctx = EdgeContext::new(PeerDeps::Skip, DevDeps::Include).with_catalogs(&catalogs);
        add_edges_from(&mut graph, root_index, &root_pkg, &ctx);

        // Edges should have resolved specs, not raw catalog: references
        let edges: HashMap<String, String> = graph
            .get_dependency_edges(root_index)
            .into_iter()
            .map(|(_, e)| (e.name.clone(), e.spec.clone()))
            .collect();

        assert_eq!(edges.get("lodash"), Some(&"^4.17.0".to_string()));
        assert_eq!(edges.get("react"), Some(&"^18.0.0".to_string()));
        assert_eq!(edges.get("tslib"), Some(&"^2.0.0".to_string()));

        // Since edges are now resolved, gather_preload_deps should find them
        let deps = gather_preload_deps(&graph, PeerDeps::Skip);
        let deps_map: HashMap<String, String> = deps.into_iter().collect();
        assert_eq!(deps_map.get("lodash"), Some(&"^4.17.0".to_string()));
        assert_eq!(deps_map.get("react"), Some(&"^18.0.0".to_string()));
        assert_eq!(deps_map.get("tslib"), Some(&"^2.0.0".to_string()));
    }

    #[test]
    fn test_edge_context_missing_catalog_keeps_raw_spec() {
        // Missing catalog entry → spec stays as raw "catalog:" (will fail at resolve time)
        let root_pkg = PackageJson::new("root", "1.0.0");
        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([(
                "missing-pkg".to_string(),
                "catalog:".to_string(),
            )])),
            ..root_pkg
        };

        let empty_catalogs: Catalogs = HashMap::new();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        let ctx = EdgeContext::new(PeerDeps::Skip, DevDeps::Include).with_catalogs(&empty_catalogs);
        add_edges_from(&mut graph, root_index, &root_pkg, &ctx);

        // Edge keeps raw spec since catalog entry is missing
        let edges: HashMap<String, String> = graph
            .get_dependency_edges(root_index)
            .into_iter()
            .map(|(_, e)| (e.name.clone(), e.spec.clone()))
            .collect();
        assert_eq!(edges.get("missing-pkg"), Some(&"catalog:".to_string()));

        // gather_preload_deps should NOT include it (not a registry spec)
        let deps = gather_preload_deps(&graph, PeerDeps::Skip);
        assert!(deps.is_empty());
    }
}
