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
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use futures::stream::{FuturesUnordered, StreamExt};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::mpsc;

#[cfg(not(target_arch = "wasm32"))]
use crate::model::manifest::FullManifest;
#[cfg(not(target_arch = "wasm32"))]
use crate::resolver::version::resolve_target_version;
#[cfg(not(target_arch = "wasm32"))]
use crate::spec::SpecStr;

#[cfg(feature = "http-tarball")]
use anyhow::Context as _;

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::manifest::NodeManifest;
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
#[cfg(target_arch = "wasm32")]
use crate::resolver::preload::{PreloadConfig, preload_manifests};
use crate::resolver::registry::{ResolveError, resolve_registry_dep};
use crate::spec::{Catalogs, PackageSpec, Protocol};
use crate::traits::progress::{BuildEvent, EventReceiver, NoopReceiver};
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
/// Sync graph mutation for a registry edge whose `ResolvedPackage` has
/// already been fetched. Mirrors the post-fetch path of
/// [`process_dependency`] without the per-variant fetch dispatch — used
/// by the channel-based main loop after pulling a manifest from the
/// preload mpsc.
///
/// Override re-resolution is intentionally NOT done here: it would
/// require an additional async fetch that the sync caller cannot
/// dispatch. Callers using overrides should fall through to
/// `process_dependency` (which handles overrides via async) for any
/// edge whose `name` matches an override rule.
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

/// Channel item: `(underlying_name, fetched FullManifest or error)`.
#[cfg(not(target_arch = "wasm32"))]
type PreloadResult = (String, anyhow::Result<Arc<FullManifest>>);

/// Spawned preload task: fetches FullManifests in parallel via
/// `service::manifest::fetch_full_manifest` (pure HTTP, no OnceMap
/// or cache coupling), walks transitive dependencies by manifest
/// content, sends each fetched manifest to main via mpsc keyed by
/// underlying package name (after `normalize_spec`).
///
/// Stateless w.r.t. shared caches — main is the sole writer. Dedup
/// is local to this task: one network fetch per underlying name
/// regardless of how many specs reference it.
#[cfg(not(target_arch = "wasm32"))]
async fn preload_to_channel(
    initial_deps: Vec<(String, String)>,
    registry_url: String,
    config: BuildDepsConfig,
    manifest_tx: mpsc::Sender<PreloadResult>,
) {
    use crate::resolver::preload::{PreloadConfig, extract_transitive_deps};
    use crate::resolver::semver::normalize_spec;
    use crate::service::{
        FetchManifestOptions, FetchManifestResult, MetadataFormat, fetch_full_manifest,
    };

    let cap = config.concurrency.max(1);
    let preload_config = PreloadConfig {
        peer_deps: config.peer_deps,
        concurrency: cap,
    };

    // Pending entries are (slot_name, spec); we normalize per pop to get
    // the underlying name to fetch. Dedup at the underlying-name layer
    // (one network fetch per package) and at the (underlying, version)
    // layer (one transitive walk per resolved version).
    let mut pending: VecDeque<(String, String)> = initial_deps.into();
    let mut seen_specs: HashSet<(String, String)> = HashSet::new();
    let mut seen_walks: HashSet<(String, String)> = HashSet::new();
    let mut name_full: HashMap<String, Arc<FullManifest>> = HashMap::new();
    let mut in_flight_names: HashSet<String> = HashSet::new();
    let mut deferred: HashMap<String, Vec<String>> = HashMap::new();
    let mut futs = FuturesUnordered::new();

    let walk_for_spec = |full: &FullManifest,
                         real_spec: &str,
                         pending: &mut VecDeque<(String, String)>,
                         seen_walks: &mut HashSet<(String, String)>| {
        let Ok(version) = resolve_target_version(full.into(), real_spec) else {
            return;
        };
        let walk_key = (full.name.clone(), version.clone());
        if !seen_walks.insert(walk_key) {
            return;
        }
        let Some(core) = full.get_core_version(&version) else {
            return;
        };
        for (n, s) in extract_transitive_deps(&core, &preload_config) {
            pending.push_back((n, s));
        }
    };

    loop {
        while futs.len() < cap {
            let Some((slot_name, spec)) = pending.pop_front() else {
                break;
            };
            let key = (slot_name.clone(), spec.clone());
            if !seen_specs.insert(key) {
                continue;
            }

            let (real_name, real_spec) = normalize_spec(&slot_name, &spec);

            // Already have FullManifest: walk transitives synchronously.
            if let Some(full) = name_full.get(&real_name).cloned() {
                walk_for_spec(&full, &real_spec, &mut pending, &mut seen_walks);
                continue;
            }

            // In-flight: defer this spec's transitive walk until fetch lands.
            if !in_flight_names.insert(real_name.clone()) {
                deferred.entry(real_name).or_default().push(real_spec);
                continue;
            }

            let url = registry_url.clone();
            let n = real_name.clone();
            let s = real_spec;
            futs.push(async move {
                let opts = FetchManifestOptions {
                    registry_url: &url,
                    name: &n,
                    format: MetadataFormat::Abbreviated,
                    etag: None,
                };
                let r = match fetch_full_manifest(opts).await {
                    Ok(FetchManifestResult::Ok(full, _etag)) => Ok(Arc::new(full)),
                    Ok(FetchManifestResult::NotModified) => {
                        Err(anyhow::anyhow!("304 Not Modified without etag context"))
                    }
                    Err(e) => Err(e),
                };
                (n, s, r)
            });
        }

        if futs.is_empty() {
            break;
        }

        let (real_name, fetch_spec, result) = futs.next().await.expect("non-empty futs");
        in_flight_names.remove(&real_name);
        match result {
            Ok(full) => {
                name_full.insert(real_name.clone(), Arc::clone(&full));
                walk_for_spec(&full, &fetch_spec, &mut pending, &mut seen_walks);
                for s in deferred.remove(&real_name).unwrap_or_default() {
                    walk_for_spec(&full, &s, &mut pending, &mut seen_walks);
                }
                if manifest_tx.send((real_name, Ok(full))).await.is_err() {
                    break; // main dropped receiver
                }
            }
            Err(e) => {
                // Send error so main can decide (optional vs hard fail).
                let _ = manifest_tx.send((real_name, Err(e))).await;
            }
        }
    }
}

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
pub async fn build_deps_with_receiver<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: RegistryClient,
    E: EventReceiver,
{
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
pub async fn build_deps_with_config<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: BuildDepsConfig,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: RegistryClient,
    E: EventReceiver,
{
    tracing::debug!(
        "Starting dependency tree build, peer_deps: {:?}, concurrency: {}, skip_preload: {}",
        config.peer_deps,
        config.concurrency,
        config.skip_preload
    );

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Native: spawn preload as concurrent task; main loop owns graph
        // and runs BFS by level, draining preload's mpsc on cache miss.
        // Single-writer pattern eliminates DashMap contention.
        mb_fetch_with_graph(graph, registry, &config, receiver).await?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        // WASM: no spawn — fall back to sequential two-phase preload + BFS.
        run_preload_phase(graph, registry, &config, receiver).await;
        run_bfs_phase(graph, registry, &config, receiver).await?;
    }

    receiver.on_event(BuildEvent::Complete {
        total_nodes: graph.graph.node_count(),
    });

    Ok(())
}

/// Native channel-based resolve: spawned preload feeds FullManifests to
/// the main loop via mpsc; main loop owns the graph + cache writes and
/// runs BFS level-by-level. Cache is keyed by **underlying** package
/// name (alias slot vs underlying package distinct), so `npm:` aliases
/// and same-named real packages don't collide on the cache.
///
/// Level barrier guarantees correctness for `npm:` alias semantics:
/// any alias edge declared at level N is fully processed (slot
/// occupied) before any level-N+1 transitive edge attempts
/// `find_compatible_node`.
#[cfg(not(target_arch = "wasm32"))]
async fn mb_fetch_with_graph<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: &BuildDepsConfig,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: RegistryClient,
    E: EventReceiver,
{
    use crate::resolver::semver::normalize_spec;

    let cap = config.concurrency.max(1);

    // Initial deps for preload: root + workspace registry edges.
    let initial_deps = gather_preload_deps(graph, config.peer_deps);
    if initial_deps.is_empty() && !graph_has_unresolved_edges(graph) {
        return Ok(());
    }

    if !initial_deps.is_empty() {
        receiver.on_event(BuildEvent::PreloadStart {
            count: initial_deps.len(),
        });
    }

    let (manifest_tx, mut manifest_rx) = mpsc::channel::<PreloadResult>(cap * 2 + 16);

    // Spawn preload concurrent with main BFS. Preload writes nothing
    // shared — sends manifests through the channel.
    let registry_url = registry.registry_url().to_string();
    let config_for_preload = config.clone();
    let preload_initial = initial_deps;
    let preload_handle = tokio::spawn(async move {
        preload_to_channel(
            preload_initial,
            registry_url,
            config_for_preload,
            manifest_tx,
        )
        .await
    });

    // Main loop's local FullManifest cache (keyed by underlying name).
    // Single-writer (this main task), so plain HashMap — no DashMap.
    let mut full_cache: HashMap<String, Arc<FullManifest>> = HashMap::new();
    // BFS edges blocked on a fetch keyed by underlying name.
    let mut waiters: HashMap<String, Vec<(NodeIndex, DependencyEdgeInfo)>> = HashMap::new();
    let mut preload_failed: HashSet<String> = HashSet::new();

    // BFS state — level-by-level expansion.
    let root_idx = graph.root_index;
    let mut current_level_nodes = vec![root_idx];

    while !current_level_nodes.is_empty() {
        let mut next_level_nodes: Vec<NodeIndex> = Vec::new();

        // Add workspace nodes to next level (mirrors old BFS).
        for node_index in &current_level_nodes {
            for (_, dep) in graph.get_dependency_edges(*node_index) {
                if dep.valid
                    && let Some(to) = dep.to
                    && let Some(n) = graph.get_node(to)
                    && n.is_workspace()
                    && *node_index == root_idx
                {
                    next_level_nodes.push(to);
                }
            }
        }

        // Collect all unresolved edges in this level into a flat queue.
        let mut level_pending: VecDeque<(NodeIndex, DependencyEdgeInfo)> = VecDeque::new();
        for node_idx in &current_level_nodes {
            let unresolved = collect_unresolved_edges(graph, *node_idx);
            if !unresolved.is_empty() {
                receiver.on_event(BuildEvent::DependencyCount {
                    count: unresolved.len(),
                });
            }
            for edge in unresolved {
                level_pending.push_back((*node_idx, edge));
            }
        }

        // Drain level: process inline if cache hit / non-registry,
        // otherwise defer until preload sends the FullManifest.
        loop {
            // Phase 1: try to drain level_pending without blocking on mpsc.
            while let Some((parent, edge)) = level_pending.pop_front() {
                receiver.on_event(BuildEvent::Resolving { name: &edge.name });

                // Non-registry edges (workspace / git / http / file): old async path.
                if !edge.spec.is_registry_spec() {
                    let processed = process_dependency(graph, registry, parent, &edge, config)
                        .await
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
                    handle_processed(
                        graph,
                        receiver,
                        parent,
                        &edge,
                        &processed,
                        &mut next_level_nodes,
                    );
                    continue;
                }

                // Registry edge: look up by underlying name.
                let (real_name, real_spec) = normalize_spec(&edge.name, &edge.spec);

                if preload_failed.contains(real_name.as_str()) {
                    if edge.edge_type == EdgeType::Optional {
                        receiver.on_event(BuildEvent::Skipped {
                            name: &edge.name,
                            spec: &edge.spec,
                        });
                        continue;
                    }
                    // Hard fail: surface the error via fallback async fetch.
                    let processed = process_dependency(graph, registry, parent, &edge, config)
                        .await
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
                    handle_processed(
                        graph,
                        receiver,
                        parent,
                        &edge,
                        &processed,
                        &mut next_level_nodes,
                    );
                    continue;
                }

                if let Some(full) = full_cache.get(real_name.as_str()).cloned() {
                    process_registry_edge(
                        graph,
                        receiver,
                        parent,
                        &edge,
                        &full,
                        &real_spec,
                        config,
                        &mut next_level_nodes,
                    );
                    continue;
                }

                // Cache miss: defer until preload sends this name.
                waiters.entry(real_name).or_default().push((parent, edge));
            }

            if waiters.is_empty() {
                break;
            }

            // Phase 2: drain mpsc until at least one waiter is satisfied.
            let resolved_one = drain_until_progress(
                &mut manifest_rx,
                &mut full_cache,
                &mut waiters,
                &mut preload_failed,
                &mut level_pending,
            )
            .await;
            if !resolved_one {
                // Preload exited unexpectedly with no remaining manifests
                // — fall back to async path for any waiting edges.
                for (_, ws) in waiters.drain() {
                    for (parent, edge) in ws {
                        let processed = process_dependency(graph, registry, parent, &edge, config)
                            .await
                            .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
                        handle_processed(
                            graph,
                            receiver,
                            parent,
                            &edge,
                            &processed,
                            &mut next_level_nodes,
                        );
                    }
                }
                break;
            }
        }

        receiver.on_event(BuildEvent::LevelComplete {
            next_level_count: next_level_nodes.len(),
        });
        current_level_nodes = next_level_nodes;
    }

    drop(manifest_rx);
    let _ = preload_handle.await;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn graph_has_unresolved_edges(graph: &DependencyGraph) -> bool {
    for idx in graph.graph.node_indices() {
        if !collect_unresolved_edges(graph, idx).is_empty() {
            return true;
        }
    }
    false
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
#[allow(clippy::too_many_arguments)]
fn process_registry_edge<E: EventReceiver>(
    graph: &mut DependencyGraph,
    receiver: &E,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    full: &FullManifest,
    real_spec: &str,
    config: &BuildDepsConfig,
    next_level: &mut Vec<NodeIndex>,
) {
    let Ok(version) = resolve_target_version(full.into(), real_spec) else {
        if edge.edge_type == EdgeType::Optional {
            receiver.on_event(BuildEvent::Skipped {
                name: &edge.name,
                spec: &edge.spec,
            });
        }
        return;
    };
    let Some(core) = full.get_core_version(&version) else {
        return;
    };
    let core_arc = Arc::new(core);
    let resolved = ResolvedPackage {
        name: edge.name.clone(),
        version: core_arc.version.clone(),
        manifest: core_arc,
    };
    receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));
    let processed = process_dependency_with_resolved(graph, parent, edge, &resolved, config);
    handle_processed(graph, receiver, parent, edge, &processed, next_level);
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
async fn drain_until_progress(
    manifest_rx: &mut mpsc::Receiver<PreloadResult>,
    full_cache: &mut HashMap<String, Arc<FullManifest>>,
    waiters: &mut HashMap<String, Vec<(NodeIndex, DependencyEdgeInfo)>>,
    preload_failed: &mut HashSet<String>,
    level_pending: &mut VecDeque<(NodeIndex, DependencyEdgeInfo)>,
) -> bool {
    while let Some((real_name, result)) = manifest_rx.recv().await {
        match result {
            Ok(full) => {
                full_cache.insert(real_name.clone(), Arc::clone(&full));
                if let Some(ws) = waiters.remove(&real_name) {
                    for entry in ws {
                        level_pending.push_back(entry);
                    }
                    return true;
                }
            }
            Err(_e) => {
                preload_failed.insert(real_name.clone());
                if let Some(ws) = waiters.remove(&real_name) {
                    for entry in ws {
                        level_pending.push_back(entry);
                    }
                    return true;
                }
            }
        }
    }
    false
}

/// Run the preload phase to warm up the cache with manifests.
#[cfg(target_arch = "wasm32")]
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
#[cfg(target_arch = "wasm32")]
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
pub async fn resolve_with_options<R, E>(
    pkg: &PackageJson,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<PackageLock, ResolveError<R::Error>>
where
    R: RegistryClient,
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
