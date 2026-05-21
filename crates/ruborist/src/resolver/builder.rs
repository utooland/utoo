//! Dependency tree builder using BFS traversal.
//!
//! This module provides the core algorithm for building a dependency graph
//! from a root package. It uses breadth-first traversal to resolve dependencies
//! level by level, with support for:
//! - Version conflict detection and nested installation
//! - Hoisting (placing packages as high as possible in the tree)
//! - Override rules
//! - Different dependency types (prod, dev, peer, optional)
//! - Demand-driven parallel manifest jobs for performance
//!
//! # Demand BFS Resolution
//!
//! The builder owns breadth-first traversal, per-run manifest cache, waiters,
//! and inflight de-duplication. Provider tasks only execute concrete manifest
//! jobs such as fetch, parse, extract, and persistence.

use futures::stream::{FuturesUnordered, StreamExt};
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "http-tarball")]
use anyhow::Context as _;

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::manifest::NodeManifest;
use crate::model::manifest::{CoreVersionManifest, FullManifest, VersionsRef};
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::resolver::registry::{ResolveError, resolve_registry_dep};
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::service::{
    ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider, MetadataFormat,
    ProjectCacheData,
};
use crate::spec::{Catalogs, PackageSpec, Protocol, SpecStr};
use crate::traits::progress::{BuildEvent, EventReceiver, NoopReceiver};
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

const DEFAULT_CONCURRENCY: usize = 128;

/// Configuration for dependency resolution.
#[derive(Debug, Clone)]
pub struct BuildDepsConfig {
    /// How to handle peer dependencies.
    pub peer_deps: PeerDeps,
    /// Maximum number of concurrent manifest jobs.
    pub concurrency: usize,
    /// Cache directory for git clones (defaults to `~/.cache/nm`)
    pub cache_dir: Option<PathBuf>,
    /// Shared dedup cache for concurrent git clone operations
    pub git_clone_cache: Arc<GitCloneCache>,
    /// Shared dedup cache for concurrent HTTP tarball fetches
    pub http_fetch_cache: Arc<HttpFetchCache>,
    /// Catalog definitions for the `catalog:` dependency protocol.
    /// Key `""` = default catalog, other keys = named catalogs.
    pub catalogs: Catalogs,
    /// Host-provided project cache used to seed the resolver-owned manifest cache.
    pub warm_project_cache: Option<ProjectCacheData>,
}

impl Default for BuildDepsConfig {
    fn default() -> Self {
        Self {
            peer_deps: PeerDeps::Skip,
            concurrency: DEFAULT_CONCURRENCY,
            cache_dir: dirs::home_dir().map(|d| d.join(".cache/nm")),
            git_clone_cache: Arc::new(GitCloneCache::new()),
            http_fetch_cache: Arc::new(HttpFetchCache::new()),
            catalogs: HashMap::new(),
            warm_project_cache: None,
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

    pub fn with_warm_project_cache(mut self, warm_project_cache: Option<ProjectCacheData>) -> Self {
        self.warm_project_cache = warm_project_cache;
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
pub async fn build_deps<R>(
    graph: &mut DependencyGraph,
    registry: &R,
    peer_deps: PeerDeps,
) -> Result<(), ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
{
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
pub async fn build_deps_with_receiver<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
    let config = BuildDepsConfig::default().with_peer_deps(peer_deps);
    build_deps_with_config(graph, registry, config, receiver).await
}

/// Build the complete dependency tree with full configuration.
///
/// This is the most flexible entry point for dependency resolution. It runs
/// demand BFS and schedules manifest jobs only when the current frontier needs
/// them.
///
/// # Arguments
/// * `graph` - The dependency graph (should have root node and initial edges)
/// * `registry` - Registry client for fetching packages
/// * `config` - Build configuration (concurrency, peer_deps, cache_dir, etc.)
/// * `receiver` - Event receiver for handling build events
///
/// # Example
/// ```ignore
/// let config = BuildDepsConfig::default()
///     .with_concurrency(50);
///
/// build_deps_with_config(&mut graph, &registry, config, &receiver).await?;
/// ```
pub async fn build_deps_with_config<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: BuildDepsConfig,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
    build_deps_with_config_output(graph, registry, config, receiver)
        .await
        .map(|_| ())
}

pub(crate) async fn build_deps_with_config_output<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: BuildDepsConfig,
    receiver: &E,
) -> Result<ResolverManifestCache, ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
    tracing::debug!(
        "Starting dependency tree build, peer_deps: {:?}, concurrency: {}",
        config.peer_deps,
        config.concurrency
    );

    let manifest_cache = run_main_loop_bfs(graph, registry, &config, receiver).await?;

    receiver.on_event(BuildEvent::Complete {
        total_nodes: graph.graph.node_count(),
    });

    Ok(manifest_cache)
}

type WaitingEdge = (NodeIndex, DependencyEdgeInfo);

type VersionKey = (String, String);

#[derive(Default)]
pub(crate) struct ResolverManifestCache {
    entries: Vec<(String, String, Arc<CoreVersionManifest>)>,
}

impl ResolverManifestCache {
    pub(crate) fn into_project_cache(self) -> ProjectCacheData {
        let mut project_cache = ProjectCacheData::default();
        for (name, spec, manifest) in self.entries {
            let version = manifest.version.clone();
            let pkg_cache = project_cache.cache.entry(name).or_default();
            pkg_cache.specs.insert(spec, version.clone());
            pkg_cache
                .manifests
                .entry(version)
                .or_insert_with(|| (*manifest).clone());
        }
        project_cache
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum FetchKey {
    Full(String),
    Version(String, String),
}

impl ManifestJob {
    fn key(&self) -> FetchKey {
        match self {
            Self::Full { name, .. } => FetchKey::Full(name.clone()),
            Self::Version { name, spec, .. } | Self::ExtractVersion { name, spec, .. } => {
                FetchKey::Version(name.clone(), spec.clone())
            }
        }
    }
}

enum FetchDone {
    Full {
        name: String,
        result: Result<ManifestFullData, String>,
    },
    Version {
        name: String,
        spec: String,
        result: Result<Arc<CoreVersionManifest>, String>,
    },
}

impl FetchDone {
    fn key(&self) -> FetchKey {
        match self {
            Self::Full { name, .. } => FetchKey::Full(name.clone()),
            Self::Version { name, spec, .. } => FetchKey::Version(name.clone(), spec.clone()),
        }
    }
}

type FetchFuture = tokio::task::JoinHandle<FetchDone>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FetchPriority {
    Demand,
    Prefetch,
}

#[derive(Default)]
struct FetchQueues {
    demand: VecDeque<ManifestJob>,
    prefetch: VecDeque<ManifestJob>,
    queued: HashMap<FetchKey, FetchPriority>,
    active: HashMap<FetchKey, FetchPriority>,
}

impl FetchQueues {
    fn enqueue(&mut self, request: ManifestJob, priority: FetchPriority) {
        let key = request.key();
        if self.active.contains_key(&key) {
            return;
        }

        match (self.queued.get(&key).copied(), priority) {
            (Some(FetchPriority::Demand), _)
            | (Some(FetchPriority::Prefetch), FetchPriority::Prefetch) => {}
            (Some(FetchPriority::Prefetch), FetchPriority::Demand) => {
                self.queued.insert(key, FetchPriority::Demand);
                self.demand.push_back(request);
            }
            (None, FetchPriority::Demand) => {
                self.queued.insert(key, FetchPriority::Demand);
                self.demand.push_back(request);
            }
            (None, FetchPriority::Prefetch) => {
                self.queued.insert(key, FetchPriority::Prefetch);
                self.prefetch.push_back(request);
            }
        }
    }

    fn complete(&mut self, key: &FetchKey) {
        self.queued.remove(key);
        self.active.remove(key);
    }

    fn pop_next(&mut self, prefetch_concurrency: usize) -> Option<ManifestJob> {
        if let Some(request) = self.pop_priority(FetchPriority::Demand) {
            return Some(request);
        }

        let prefetch_concurrency = if self
            .queued
            .values()
            .any(|priority| *priority == FetchPriority::Demand)
        {
            prefetch_concurrency
        } else {
            usize::MAX
        };

        if self.active_prefetches() >= prefetch_concurrency {
            return None;
        }

        self.pop_priority(FetchPriority::Prefetch)
    }

    fn pop_priority(&mut self, priority: FetchPriority) -> Option<ManifestJob> {
        loop {
            let request = match priority {
                FetchPriority::Demand => self.demand.pop_front(),
                FetchPriority::Prefetch => self.prefetch.pop_front(),
            }?;
            let key = request.key();
            if self.queued.get(&key).copied() != Some(priority) {
                continue;
            }
            self.queued.remove(&key);
            self.active.insert(key, priority);
            return Some(request);
        }
    }

    fn active_prefetches(&self) -> usize {
        self.active
            .values()
            .filter(|priority| **priority == FetchPriority::Prefetch)
            .count()
    }
}

fn prefetch_concurrency_limit(concurrency: usize) -> usize {
    (concurrency / 4).max(1)
}

#[derive(Default)]
struct ManifestState {
    full_cache: HashMap<String, Arc<FullManifest>>,
    versions_cache: HashMap<String, Arc<crate::service::VersionsInfo>>,
    version_cache: HashMap<VersionKey, Arc<CoreVersionManifest>>,
    full_waiters: HashMap<String, Vec<WaitingEdge>>,
    version_waiters: HashMap<VersionKey, Vec<WaitingEdge>>,
    full_failures: HashMap<String, String>,
    version_failures: HashMap<VersionKey, String>,
    fetch_queues: FetchQueues,
}

impl ManifestState {
    fn with_warm_project_cache(warm: Option<&ProjectCacheData>) -> Self {
        let mut state = Self::default();
        let Some(warm) = warm else {
            return state;
        };
        for (name, pkg_cache) in &warm.cache {
            for (spec, version) in &pkg_cache.specs {
                let Some(manifest) = pkg_cache.manifests.get(version) else {
                    continue;
                };
                let manifest = Arc::new(manifest.clone());
                state
                    .version_cache
                    .insert((name.clone(), spec.clone()), Arc::clone(&manifest));
                state
                    .version_cache
                    .entry((name.clone(), version.clone()))
                    .or_insert(manifest);
            }
        }
        state
    }

    fn into_resolver_cache(self) -> ResolverManifestCache {
        ResolverManifestCache {
            entries: self
                .version_cache
                .into_iter()
                .map(|((name, spec), manifest)| (name, spec, manifest))
                .collect(),
        }
    }

    fn schedule_registry_fetch(
        &mut self,
        name: String,
        spec: String,
        supports_semver: bool,
        priority: FetchPriority,
    ) {
        let (real_name, real_spec) = normalize_spec(&name, &spec);
        if supports_semver {
            let key = (real_name, real_spec);
            if self.version_cache.contains_key(&key) || self.version_failures.contains_key(&key) {
                return;
            }
            self.fetch_queues.enqueue(
                ManifestJob::Version {
                    name: key.0.clone(),
                    spec: key.1.clone(),
                    fetch_spec: key.1,
                    format: version_metadata_format(supports_semver),
                },
                priority,
            );
        } else {
            if self.full_cache.contains_key(&real_name)
                || self.versions_cache.contains_key(&real_name)
                || self.full_failures.contains_key(&real_name)
            {
                return;
            }
            self.fetch_queues.enqueue(
                ManifestJob::Full {
                    name: real_name,
                    spec: Some(real_spec),
                },
                priority,
            );
        }
    }

    fn enqueue_version_extract(&mut self, name: String, version: String, full: Arc<FullManifest>) {
        self.fetch_queues.enqueue(
            ManifestJob::ExtractVersion {
                name,
                spec: version.clone(),
                version,
                full,
            },
            FetchPriority::Demand,
        );
    }

    fn enqueue_version_fetch(&mut self, name: String, fetch_spec: String, supports_semver: bool) {
        self.fetch_queues.enqueue(
            ManifestJob::Version {
                name,
                spec: fetch_spec.clone(),
                fetch_spec,
                format: version_metadata_format(supports_semver),
            },
            FetchPriority::Demand,
        );
    }

    fn schedule_transitive_prefetches(
        &mut self,
        manifest: &CoreVersionManifest,
        peer_deps: PeerDeps,
        supports_semver: bool,
    ) {
        for (name, spec) in collect_registry_prefetches(manifest, peer_deps) {
            self.schedule_registry_fetch(name, spec, supports_semver, FetchPriority::Prefetch);
        }
    }

    fn apply_fetch_result(
        &mut self,
        done: FetchDone,
        supports_semver: bool,
        peer_deps: PeerDeps,
        level_pending: &mut VecDeque<WaitingEdge>,
    ) {
        let done_key = done.key();
        self.fetch_queues.complete(&done_key);

        match done {
            FetchDone::Full { name, result } => {
                match result {
                    Ok(ManifestFullData::Full {
                        manifest: full,
                        speculative,
                    }) => {
                        if let Some((resolved_spec, manifest)) = speculative {
                            self.version_cache
                                .insert((name.clone(), resolved_spec), Arc::clone(&manifest));
                            self.version_cache
                                .entry((name.clone(), manifest.version.clone()))
                                .or_insert_with(|| Arc::clone(&manifest));
                            self.schedule_transitive_prefetches(
                                &manifest,
                                peer_deps,
                                supports_semver,
                            );
                        }
                        self.full_cache.insert(name.clone(), full);
                    }
                    Ok(ManifestFullData::Versions(versions)) => {
                        self.versions_cache.insert(name.clone(), versions);
                    }
                    Err(e) => {
                        self.full_failures.insert(name.clone(), e);
                    }
                }
                if let Some(waiters) = self.full_waiters.remove(&name) {
                    level_pending.extend(waiters);
                }
            }
            FetchDone::Version { name, spec, result } => {
                let key = (name, spec);
                match result {
                    Ok(manifest) => {
                        self.version_cache
                            .insert(key.clone(), Arc::clone(&manifest));
                        self.version_cache
                            .entry((key.0.clone(), manifest.version.clone()))
                            .or_insert_with(|| Arc::clone(&manifest));
                        self.schedule_transitive_prefetches(&manifest, peer_deps, supports_semver);
                    }
                    Err(e) => {
                        self.version_failures.insert(key.clone(), e);
                    }
                }
                if let Some(waiters) = self.version_waiters.remove(&key) {
                    level_pending.extend(waiters);
                }
            }
        }
    }
}

fn version_metadata_format(supports_semver: bool) -> MetadataFormat {
    if supports_semver {
        MetadataFormat::Abbreviated
    } else {
        MetadataFormat::Complete
    }
}

fn registry_error<E>(message: impl Into<String>) -> ResolveError<E>
where
    E: From<RegistryError>,
{
    ResolveError::Registry(RegistryError(anyhow::anyhow!(message.into())).into())
}

async fn fetch_registry_manifest_inner<R>(registry: R, request: ManifestJob) -> FetchDone
where
    R: ManifestProvider,
{
    let key = request.key();
    match registry.execute_manifest_job(request).await {
        Ok(done) => match done {
            ManifestJobDone::Full { name, data } => FetchDone::Full {
                name,
                result: Ok(data),
            },
            ManifestJobDone::Version {
                name,
                spec,
                manifest,
            } => FetchDone::Version {
                name,
                spec,
                result: Ok(manifest),
            },
        },
        Err(error) => match key {
            FetchKey::Full(name) => FetchDone::Full {
                name,
                result: Err(format!("{error:#}")),
            },
            FetchKey::Version(name, spec) => FetchDone::Version {
                name,
                spec,
                result: Err(format!("{error:#}")),
            },
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_registry_manifest<R>(registry: R, request: ManifestJob) -> FetchFuture
where
    R: ManifestProvider,
    R::Error: Send,
{
    tokio::spawn(fetch_registry_manifest_inner(registry, request))
}

#[cfg(target_arch = "wasm32")]
fn fetch_registry_manifest<R>(registry: R, request: ManifestJob) -> FetchFuture
where
    R: ManifestProvider,
{
    tokio::task::spawn_local(fetch_registry_manifest_inner(registry, request))
}

fn pump_fetches<R>(
    fetches: &mut FuturesUnordered<FetchFuture>,
    fetch_queues: &mut FetchQueues,
    registry: &R,
    concurrency: usize,
) where
    R: ManifestProvider,
    R::Error: Send,
{
    let prefetch_concurrency = prefetch_concurrency_limit(concurrency);
    while fetches.len() < concurrency {
        let Some(request) = fetch_queues.pop_next(prefetch_concurrency) else {
            break;
        };
        fetches.push(fetch_registry_manifest(registry.clone(), request));
    }
}

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

async fn handle_resolved_registry_manifest<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    receiver: &E,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    manifest: Arc<CoreVersionManifest>,
    config: &BuildDepsConfig,
) -> Result<ProcessResult, ResolveError<R::Error>>
where
    R: RegistryClient,
    E: EventReceiver,
{
    let resolved = ResolvedPackage {
        name: edge.name.clone(),
        version: manifest.version.clone(),
        manifest,
    };

    let processed = if graph
        .check_override(parent, &edge.name, Some(&resolved.version))
        .is_some()
    {
        process_dependency(graph, registry, parent, edge, config)
            .await
            .map_err(|inner| chain_err(graph, parent, edge, inner))?
    } else {
        receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));
        process_dependency_with_resolved(graph, parent, edge, &resolved, config)
    };

    Ok(processed)
}

fn resolve_version_from_versions<RE>(
    edge: &DependencyEdgeInfo,
    package_name: &str,
    versions: VersionsRef<'_>,
    real_spec: &str,
) -> Result<Option<String>, ResolveError<RE>> {
    if versions.versions.is_empty() {
        if edge.edge_type == EdgeType::Optional {
            return Ok(None);
        }
        return Err(ResolveError::NoVersions(package_name.to_string()));
    }

    let version = match resolve_target_version(versions, real_spec) {
        Ok(version) => version,
        Err(_) if edge.edge_type == EdgeType::Optional => return Ok(None),
        Err(e) => {
            return Err(ResolveError::Version(format!(
                "{}@{}: {}",
                edge.name, real_spec, e
            )));
        }
    };
    Ok(Some(version))
}

fn resolve_version_from_full_manifest<RE>(
    edge: &DependencyEdgeInfo,
    full: &FullManifest,
    real_spec: &str,
) -> Result<Option<String>, ResolveError<RE>> {
    resolve_version_from_versions(edge, &full.name, full.into(), real_spec)
}

fn collect_registry_prefetches(
    manifest: &CoreVersionManifest,
    peer_deps: PeerDeps,
) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    manifest.for_each_dep(peer_deps, DevDeps::Exclude, |_, name, spec| {
        if spec.is_registry_spec() {
            deps.push((name.to_string(), spec.to_string()));
        }
    });
    deps
}

async fn run_main_loop_bfs<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: &BuildDepsConfig,
    receiver: &E,
) -> Result<ResolverManifestCache, ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
    let supports_semver = registry.supports_semver_resolution();
    let concurrency = config.concurrency.max(1);

    let mut state = ManifestState::with_warm_project_cache(config.warm_project_cache.as_ref());
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
            pump_fetches(&mut fetches, &mut state.fetch_queues, registry, concurrency);

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
                    if let Some(error) = state.version_failures.get(&key) {
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

                    if let Some(manifest) = state.version_cache.get(&key).cloned() {
                        let processed = handle_resolved_registry_manifest(
                            graph, registry, receiver, parent, &edge, manifest, config,
                        )
                        .await?;
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

                    state
                        .version_waiters
                        .entry(key.clone())
                        .or_default()
                        .push((parent, edge));
                    state.schedule_registry_fetch(
                        key.0,
                        key.1,
                        supports_semver,
                        FetchPriority::Demand,
                    );
                } else {
                    if let Some(error) = state.full_failures.get(&real_name) {
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

                    let version_key = (real_name.clone(), real_spec.clone());
                    if let Some(error) = state.version_failures.get(&version_key) {
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

                    if let Some(manifest) = state.version_cache.get(&version_key).cloned() {
                        let processed = handle_resolved_registry_manifest(
                            graph, registry, receiver, parent, &edge, manifest, config,
                        )
                        .await?;
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

                    if let Some(full) = state.full_cache.get(&real_name).cloned() {
                        let Some(resolved_version) =
                            resolve_version_from_full_manifest::<R::Error>(
                                &edge, &full, &real_spec,
                            )
                            .map_err(|inner| chain_err(graph, parent, &edge, inner))?
                        else {
                            receiver.on_event(BuildEvent::Skipped {
                                name: &edge.name,
                                spec: &edge.spec,
                            });
                            continue;
                        };

                        let exact_key = (real_name.clone(), resolved_version.clone());
                        if let Some(error) = state.version_failures.get(&exact_key) {
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

                        if let Some(manifest) = state.version_cache.get(&exact_key).cloned() {
                            state
                                .version_cache
                                .insert(version_key, Arc::clone(&manifest));
                            let processed = handle_resolved_registry_manifest(
                                graph, registry, receiver, parent, &edge, manifest, config,
                            )
                            .await?;
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

                        state
                            .version_waiters
                            .entry(exact_key)
                            .or_default()
                            .push((parent, edge));
                        state.enqueue_version_extract(real_name, resolved_version, full);
                        continue;
                    }

                    if let Some(versions) = state.versions_cache.get(&real_name).cloned() {
                        let Some(resolved_version) = resolve_version_from_versions::<R::Error>(
                            &edge,
                            &real_name,
                            (&*versions).into(),
                            &real_spec,
                        )
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?
                        else {
                            receiver.on_event(BuildEvent::Skipped {
                                name: &edge.name,
                                spec: &edge.spec,
                            });
                            continue;
                        };

                        let exact_key = (real_name.clone(), resolved_version.clone());
                        if let Some(error) = state.version_failures.get(&exact_key) {
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

                        if let Some(manifest) = state.version_cache.get(&exact_key).cloned() {
                            state
                                .version_cache
                                .insert(version_key, Arc::clone(&manifest));
                            let processed = handle_resolved_registry_manifest(
                                graph, registry, receiver, parent, &edge, manifest, config,
                            )
                            .await?;
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

                        state
                            .version_waiters
                            .entry(exact_key)
                            .or_default()
                            .push((parent, edge));
                        state.enqueue_version_fetch(real_name, resolved_version, supports_semver);
                        continue;
                    }

                    state
                        .full_waiters
                        .entry(real_name.clone())
                        .or_default()
                        .push((parent, edge));
                    state.schedule_registry_fetch(
                        real_name,
                        real_spec,
                        supports_semver,
                        FetchPriority::Demand,
                    );
                }

                pump_fetches(&mut fetches, &mut state.fetch_queues, registry, concurrency);
            }

            loop {
                let ready = std::future::poll_fn(|cx| match fetches.poll_next_unpin(cx) {
                    std::task::Poll::Ready(done) => std::task::Poll::Ready(done),
                    std::task::Poll::Pending => std::task::Poll::Ready(None),
                })
                .await;
                let Some(done) = ready else {
                    break;
                };
                let done = done.map_err(|e| {
                    registry_error::<R::Error>(format!("manifest fetch task failed: {e}"))
                })?;

                state.apply_fetch_result(
                    done,
                    supports_semver,
                    config.peer_deps,
                    &mut level_pending,
                );
            }

            if !level_pending.is_empty() {
                continue;
            }

            if !state.full_waiters.is_empty() || !state.version_waiters.is_empty() {
                pump_fetches(&mut fetches, &mut state.fetch_queues, registry, concurrency);
            }

            if state.full_waiters.is_empty() && state.version_waiters.is_empty() {
                break;
            }

            let Some(done) = fetches.next().await else {
                let mut fallback = Vec::new();
                for (_, waiters) in state.full_waiters.drain() {
                    fallback.extend(waiters);
                }
                for (_, waiters) in state.version_waiters.drain() {
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

            state.apply_fetch_result(done, supports_semver, config.peer_deps, &mut level_pending);
        }

        receiver.on_event(BuildEvent::LevelComplete {
            next_level_count: next_level.len(),
        });
        current_level = next_level;
    }

    Ok(state.into_resolver_cache())
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
pub async fn resolve<R>(
    pkg: &PackageJson,
    registry: &R,
) -> Result<PackageLock, ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
{
    resolve_with_options(pkg, registry, PeerDeps::Include, &NoopReceiver).await
}

/// Build package-lock.json with options.
///
/// # Arguments
/// * `pkg` - The root package.json
/// * `registry` - Registry client for fetching packages
/// * `peer_deps` - How to handle peer dependencies
/// * `receiver` - Event receiver for progress tracking
pub async fn resolve_with_options<R, E>(
    pkg: &PackageJson,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<PackageLock, ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::manifest::CoreVersionManifest;
    use crate::traits::registry::mock::{MockError, MockRegistryClient};

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

    #[derive(Clone)]
    struct CountingRegistry {
        inner: MockRegistryClient,
        shared_version_jobs: Arc<AtomicUsize>,
    }

    impl crate::traits::registry::RegistryClient for CountingRegistry {
        type Error = MockError;

        async fn fetch_full_manifest(&self, name: &str) -> Result<Arc<FullManifest>, Self::Error> {
            self.inner.fetch_full_manifest(name).await
        }
    }

    #[async_trait::async_trait]
    impl ManifestProvider for CountingRegistry {
        async fn execute_manifest_job(
            &self,
            job: ManifestJob,
        ) -> Result<ManifestJobDone, Self::Error> {
            if matches!(
                &job,
                ManifestJob::Full { name, .. }
                    | ManifestJob::Version { name, .. }
                    | ManifestJob::ExtractVersion { name, .. }
                    if name == "shared"
            ) {
                self.shared_version_jobs.fetch_add(1, Ordering::Relaxed);
            }
            self.inner.execute_manifest_job(job).await
        }
    }

    #[tokio::test]
    async fn test_non_semver_exact_version_extract_single_flight() {
        let mut inner = MockRegistryClient::new();
        inner.add_package(
            "a",
            "1.0.0",
            create_version_manifest_with_deps("a", "1.0.0", vec![("shared", "^1.0.0")]),
        );
        inner.add_package(
            "b",
            "1.0.0",
            create_version_manifest_with_deps("b", "1.0.0", vec![("shared", "~1.2.0")]),
        );
        inner.add_package(
            "shared",
            "1.2.3",
            create_version_manifest("shared", "1.2.3"),
        );

        let shared_version_jobs = Arc::new(AtomicUsize::new(0));
        let registry = CountingRegistry {
            inner,
            shared_version_jobs: Arc::clone(&shared_version_jobs),
        };
        let pkg = PackageJson {
            dependencies: Some(HashMap::from([
                ("a".to_string(), "1.0.0".to_string()),
                ("b".to_string(), "1.0.0".to_string()),
            ])),
            ..PackageJson::new("test-project", "1.0.0")
        };

        let lock = resolve(&pkg, &registry).await.unwrap();

        assert!(lock.packages.contains_key("node_modules/shared"));
        assert_eq!(shared_version_jobs.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_schedule_registry_fetch_dedupes_semver_request() {
        let mut state = ManifestState::default();

        state.schedule_registry_fetch(
            "pkg".to_string(),
            "^1.0.0".to_string(),
            true,
            FetchPriority::Demand,
        );
        state.schedule_registry_fetch(
            "pkg".to_string(),
            "^1.0.0".to_string(),
            true,
            FetchPriority::Demand,
        );

        assert!(
            state
                .fetch_queues
                .queued
                .contains_key(&FetchKey::Version("pkg".to_string(), "^1.0.0".to_string()))
        );
        match state
            .fetch_queues
            .pop_next(prefetch_concurrency_limit(64))
            .unwrap()
        {
            ManifestJob::Version {
                name,
                spec,
                fetch_spec,
                format,
            } => {
                assert_eq!(name, "pkg");
                assert_eq!(spec, "^1.0.0");
                assert_eq!(fetch_spec, "^1.0.0");
                assert!(matches!(format, MetadataFormat::Abbreviated));
            }
            _ => panic!("expected version fetch request"),
        }
    }

    #[test]
    fn test_fetch_queues_prioritize_demand_over_prefetch() {
        let mut fetch_queues = FetchQueues::default();
        fetch_queues.enqueue(
            ManifestJob::Full {
                name: "prefetch".to_string(),
                spec: None,
            },
            FetchPriority::Prefetch,
        );
        fetch_queues.enqueue(
            ManifestJob::Version {
                name: "demand".to_string(),
                spec: "^1.0.0".to_string(),
                fetch_spec: "^1.0.0".to_string(),
                format: MetadataFormat::Abbreviated,
            },
            FetchPriority::Demand,
        );

        assert_eq!(
            fetch_queues
                .pop_next(prefetch_concurrency_limit(64))
                .unwrap()
                .key(),
            FetchKey::Version("demand".to_string(), "^1.0.0".to_string())
        );
        assert_eq!(
            fetch_queues
                .pop_next(prefetch_concurrency_limit(64))
                .unwrap()
                .key(),
            FetchKey::Full("prefetch".to_string())
        );
    }

    #[test]
    fn test_fetch_queues_promotes_prefetch_to_demand() {
        let mut fetch_queues = FetchQueues::default();
        fetch_queues.enqueue(
            ManifestJob::Full {
                name: "pkg".to_string(),
                spec: None,
            },
            FetchPriority::Prefetch,
        );
        fetch_queues.enqueue(
            ManifestJob::Full {
                name: "pkg".to_string(),
                spec: None,
            },
            FetchPriority::Demand,
        );

        let key = FetchKey::Full("pkg".to_string());
        assert_eq!(fetch_queues.queued.get(&key), Some(&FetchPriority::Demand));
        assert_eq!(
            fetch_queues
                .pop_next(prefetch_concurrency_limit(64))
                .unwrap()
                .key(),
            key
        );
        assert_eq!(
            fetch_queues.active.get(&FetchKey::Full("pkg".to_string())),
            Some(&FetchPriority::Demand)
        );
        assert!(
            fetch_queues
                .pop_next(prefetch_concurrency_limit(64))
                .is_none()
        );
    }

    #[test]
    fn test_prefetch_concurrency_limit_tracks_fetch_concurrency() {
        assert_eq!(prefetch_concurrency_limit(1), 1);
        assert_eq!(prefetch_concurrency_limit(3), 1);
        assert_eq!(prefetch_concurrency_limit(8), 2);
    }

    #[test]
    fn test_apply_fetch_result_caches_exact_version_and_wakes_waiters() {
        let mut state = ManifestState {
            version_waiters: HashMap::from([(
                ("pkg".to_string(), "^1.0.0".to_string()),
                vec![(
                    NodeIndex::new(0),
                    DependencyEdgeInfo {
                        edge_id: petgraph::graph::EdgeIndex::new(0),
                        name: "pkg".to_string(),
                        spec: "^1.0.0".to_string(),
                        edge_type: EdgeType::Prod,
                    },
                )],
            )]),
            ..Default::default()
        };
        state.fetch_queues.active.insert(
            FetchKey::Version("pkg".to_string(), "^1.0.0".to_string()),
            FetchPriority::Demand,
        );
        let mut level_pending = std::collections::VecDeque::new();
        let manifest = Arc::new(create_version_manifest("pkg", "1.2.3"));

        state.apply_fetch_result(
            FetchDone::Version {
                name: "pkg".to_string(),
                spec: "^1.0.0".to_string(),
                result: Ok(manifest),
            },
            true,
            PeerDeps::Skip,
            &mut level_pending,
        );

        assert!(
            state
                .version_cache
                .contains_key(&("pkg".to_string(), "^1.0.0".to_string()))
        );
        assert!(
            state
                .version_cache
                .contains_key(&("pkg".to_string(), "1.2.3".to_string()))
        );
        assert!(state.version_waiters.is_empty());
        assert!(state.fetch_queues.queued.is_empty());
        assert!(state.fetch_queues.active.is_empty());
        assert_eq!(level_pending.len(), 1);
    }

    #[test]
    fn test_apply_fetch_result_prefetches_transitive_registry_deps() {
        let mut state = ManifestState::default();
        state.fetch_queues.active.insert(
            FetchKey::Version("pkg".to_string(), "^1.0.0".to_string()),
            FetchPriority::Demand,
        );
        let mut level_pending = std::collections::VecDeque::new();
        let manifest = Arc::new(create_version_manifest_with_deps(
            "pkg",
            "1.2.3",
            vec![("dep", "^1.0.0"), ("local", "file:../local")],
        ));

        state.apply_fetch_result(
            FetchDone::Version {
                name: "pkg".to_string(),
                spec: "^1.0.0".to_string(),
                result: Ok(manifest),
            },
            true,
            PeerDeps::Skip,
            &mut level_pending,
        );

        assert!(
            state
                .fetch_queues
                .queued
                .contains_key(&FetchKey::Version("dep".to_string(), "^1.0.0".to_string()))
        );
        assert!(!state.fetch_queues.queued.contains_key(&FetchKey::Version(
            "local".to_string(),
            "file:../local".to_string()
        )));
        match state
            .fetch_queues
            .pop_next(prefetch_concurrency_limit(64))
            .unwrap()
        {
            ManifestJob::Version {
                name,
                spec,
                fetch_spec,
                format,
            } => {
                assert_eq!(name, "dep");
                assert_eq!(spec, "^1.0.0");
                assert_eq!(fetch_spec, "^1.0.0");
                assert!(matches!(format, MetadataFormat::Abbreviated));
            }
            _ => panic!("expected version prefetch request"),
        }
    }

    #[test]
    fn test_apply_fetch_result_caches_versions_and_wakes_waiters() {
        let mut state = ManifestState {
            full_waiters: HashMap::from([(
                "pkg".to_string(),
                vec![(
                    NodeIndex::new(0),
                    DependencyEdgeInfo {
                        edge_id: petgraph::graph::EdgeIndex::new(0),
                        name: "pkg".to_string(),
                        spec: "^1.0.0".to_string(),
                        edge_type: EdgeType::Prod,
                    },
                )],
            )]),
            ..Default::default()
        };
        state
            .fetch_queues
            .active
            .insert(FetchKey::Full("pkg".to_string()), FetchPriority::Demand);
        let mut level_pending = std::collections::VecDeque::new();
        let versions = Arc::new(crate::service::VersionsInfo {
            versions: crate::service::Versions {
                version_list: vec!["1.2.3".to_string()],
                dist_tags: HashMap::from([("latest".to_string(), "1.2.3".to_string())]),
            },
            etag: Some("etag".to_string()),
            last_updated: 1,
        });

        state.apply_fetch_result(
            FetchDone::Full {
                name: "pkg".to_string(),
                result: Ok(ManifestFullData::Versions(versions)),
            },
            false,
            PeerDeps::Skip,
            &mut level_pending,
        );

        assert!(state.full_cache.is_empty());
        assert!(state.versions_cache.contains_key("pkg"));
        assert!(state.full_waiters.is_empty());
        assert!(state.fetch_queues.queued.is_empty());
        assert!(state.fetch_queues.active.is_empty());
        assert_eq!(level_pending.len(), 1);
    }

    #[test]
    fn test_apply_fetch_result_caches_speculative_full_extract() {
        let mut state = ManifestState::default();
        state
            .fetch_queues
            .active
            .insert(FetchKey::Full("pkg".to_string()), FetchPriority::Demand);
        let mut level_pending = std::collections::VecDeque::new();
        let full = Arc::new(FullManifest {
            name: "pkg".to_string(),
            versions: vec!["1.2.3".to_string()],
            ..Default::default()
        });
        let manifest = Arc::new(create_version_manifest_with_deps(
            "pkg",
            "1.2.3",
            vec![("dep", "^1.0.0")],
        ));

        state.apply_fetch_result(
            FetchDone::Full {
                name: "pkg".to_string(),
                result: Ok(ManifestFullData::Full {
                    manifest: full,
                    speculative: Some(("^1.0.0".to_string(), manifest)),
                }),
            },
            false,
            PeerDeps::Skip,
            &mut level_pending,
        );

        assert!(state.full_cache.contains_key("pkg"));
        assert!(
            state
                .version_cache
                .contains_key(&("pkg".to_string(), "^1.0.0".to_string()))
        );
        assert!(
            state
                .version_cache
                .contains_key(&("pkg".to_string(), "1.2.3".to_string()))
        );
        assert!(
            state
                .fetch_queues
                .queued
                .contains_key(&FetchKey::Full("dep".to_string()))
        );
    }

    #[test]
    fn test_enqueue_version_fetch_uses_exact_key() {
        let mut state = ManifestState::default();

        state.enqueue_version_fetch("pkg".to_string(), "1.2.3".to_string(), false);
        state.enqueue_version_fetch("pkg".to_string(), "1.2.3".to_string(), false);

        assert!(
            state
                .fetch_queues
                .queued
                .contains_key(&FetchKey::Version("pkg".to_string(), "1.2.3".to_string()))
        );
        match state
            .fetch_queues
            .pop_next(prefetch_concurrency_limit(64))
            .unwrap()
        {
            ManifestJob::Version {
                name,
                spec,
                fetch_spec,
                format,
            } => {
                assert_eq!(name, "pkg");
                assert_eq!(spec, "1.2.3");
                assert_eq!(fetch_spec, "1.2.3");
                assert!(matches!(format, MetadataFormat::Complete));
            }
            _ => panic!("expected version fetch request"),
        }
    }

    #[test]
    fn test_enqueue_version_extract_uses_exact_key() {
        let mut state = ManifestState::default();
        let full = Arc::new(FullManifest::default());

        state.enqueue_version_extract("pkg".to_string(), "1.2.3".to_string(), Arc::clone(&full));
        state.enqueue_version_extract("pkg".to_string(), "1.2.3".to_string(), full);

        assert!(
            state
                .fetch_queues
                .queued
                .contains_key(&FetchKey::Version("pkg".to_string(), "1.2.3".to_string()))
        );
        match state
            .fetch_queues
            .pop_next(prefetch_concurrency_limit(64))
            .unwrap()
        {
            ManifestJob::ExtractVersion {
                name,
                spec,
                version,
                ..
            } => {
                assert_eq!(name, "pkg");
                assert_eq!(spec, "1.2.3");
                assert_eq!(version, "1.2.3");
            }
            _ => panic!("expected version extract request"),
        }
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
    }
}
