//! Dependency tree builder.
//!
//! This module provides the core algorithm for building a dependency graph
//! from a root package, with support for:
//! - Version conflict detection and nested installation
//! - Hoisting (placing packages as high as possible in the tree)
//! - Override rules
//! - Different dependency types (prod, dev, peer, optional)
//!
//! Resolution runs through the demand-driven BFS loop in [`super::demand`]:
//! a single level-by-level traversal that schedules manifest fetches on
//! demand (no separate preload phase), overlapping network I/O with graph
//! construction.

use petgraph::graph::{EdgeIndex, NodeIndex};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "http-tarball")]
use anyhow::Context as _;

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::manifest::CoreVersionManifest;
#[cfg(feature = "http-tarball")]
use crate::model::manifest::NodeManifest;
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::resolver::demand::{ManifestState, ResolverManifestCache, run_main_loop_bfs};
use crate::resolver::registry::{ResolveError, resolve_registry_dep};
use crate::service::{ManifestProvider, ProjectCacheData};
use crate::spec::{Catalogs, PackageSpec, Protocol};
use crate::traits::progress::{BuildEvent, EventReceiver, NoopReceiver};
use crate::traits::registry::ResolvedPackage;

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

/// Default number of concurrent manifest fetches for the demand resolver.
const DEFAULT_CONCURRENCY: usize = 128;

/// Configuration for dependency resolution.
#[derive(Debug, Clone)]
pub struct BuildDepsConfig {
    /// How to handle peer dependencies.
    pub peer_deps: PeerDeps,
    /// Maximum number of concurrent manifest fetches.
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
    /// Host-provided project cache used to seed the resolver-owned manifest
    /// cache. Consumed by the demand mainloop.
    pub project_cache: Option<ProjectCacheData>,
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
            project_cache: None,
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

    /// Seed the resolver-owned manifest cache with a host-provided project cache.
    pub fn with_project_cache(mut self, project_cache: Option<ProjectCacheData>) -> Self {
        self.project_cache = project_cache;
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

/// Attach a workspace member to the root of `graph`: add its workspace node and
/// link node physically under root, mark root's dependency edge on it resolved,
/// and seed the member's own dependency edges via `edge_ctx`.
///
/// Shared by the install graph setup in `service::api` and pm's
/// workspace-topology builder so the workspace-node shape is defined once.
pub fn add_workspace_member(
    graph: &mut DependencyGraph,
    root_index: NodeIndex,
    name: &str,
    path: PathBuf,
    pkg: &PackageJson,
    edge_ctx: &EdgeContext,
) {
    let workspace_node = PackageNode::workspace_from_package_json(path.clone(), pkg.clone());
    let workspace_index = graph.add_node(workspace_node);

    let link_node = PackageNode::link_from_package_json(path, pkg.clone());
    let link_index = graph.add_node(link_node);

    graph.add_physical_edge(root_index, workspace_index);
    graph.add_physical_edge(root_index, link_index);

    let dep_edge_id = graph.add_dependency_edge(root_index, name, &pkg.version, EdgeType::Prod);
    graph.mark_dependency_resolved(dep_edge_id, workspace_index);

    add_edges_from(graph, workspace_index, pkg, edge_ctx);
}

/// Resolve every importer-declared `workspace:` edge against the attached
/// workspace members — to run AFTER all members are added (member→member
/// edges can reference members attached later) and BEFORE the BFS.
///
/// `workspace:` specs only legally come from source-declared manifests (root
/// and workspace members), and all of those edges exist by the end of graph
/// initialisation — so they can be settled here in one pass and the resolver
/// driver never sees a `workspace:` edge at all. Any `workspace:` spec that
/// still reaches the BFS afterwards is a declaration anomaly (importer naming
/// a missing member, or a spec leaked inside a published manifest), handled
/// by the optional-skip / prod-error policy in `process_dependency`.
pub fn resolve_workspace_member_edges(graph: &mut DependencyGraph) {
    let root_index = graph.root_index;
    // Maintained by `DependencyGraph::add_node` — no member-set rebuild here.
    let members = graph.workspace_members().clone();
    if members.is_empty() {
        return;
    }

    let importers: Vec<NodeIndex> = std::iter::once(root_index)
        .chain(members.values().copied())
        .collect();
    for importer in importers {
        let pending: Vec<(EdgeIndex, String)> = graph
            .get_dependency_edges(importer)
            .into_iter()
            .filter(|(_, dep)| !dep.valid && dep.spec.starts_with("workspace:"))
            .map(|(edge_id, dep)| (edge_id, dep.name.clone()))
            .collect();
        for (edge_id, name) in pending {
            if let Some(&target) = members.get(&name) {
                graph.mark_dependency_resolved(edge_id, target);
            }
            // Missing member: leave the edge unresolved — the BFS terminal
            // arm applies the optional-skip / prod-error policy with the
            // dependency chain attached.
        }
    }
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

/// Assign dependency types (prod/dev/optional/peer) across the whole graph.
///
/// This is the single source of truth for node types: the BFS only builds graph
/// structure and never touches types, so this pass — run once after the tree is
/// complete — cannot observe a half-built tree and therefore can't leave stale
/// `dev` marks on the subtree of a node that turns out to be prod (the bug that
/// motivated centralizing it here).
///
/// It applies the per-edge propagation rules ([`update_node_type_from_edge`])
/// over every resolved edge, starting from the importer roots (whose
/// `is_root`/edge type seeds their direct deps) and re-queuing a node's children
/// whenever its flags change, until nothing changes.
///
/// Termination relies on the type lattice being monotone: each node's flags only
/// move "up" (dev/optional/peer may be set once, then `is_prod` is absorbing —
/// once set it clears the others and the `!is_prod` guards freeze the node), so
/// every node changes O(1) times.
pub fn compute_node_types(graph: &mut DependencyGraph) {
    use std::collections::VecDeque;

    // Seed only the importers (root + workspace members); their edge types seed
    // their direct deps, and the fixpoint re-queues every changed node's
    // children, so the whole reachable graph is covered without visiting leaves
    // that have nothing to propagate.
    let mut queued: HashSet<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|&i| {
            graph
                .get_node(i)
                .is_some_and(|n| n.is_root() || n.is_workspace())
        })
        .collect();
    let mut worklist: VecDeque<NodeIndex> = queued.iter().copied().collect();

    while let Some(src) = worklist.pop_front() {
        queued.remove(&src);

        // Snapshot resolved out-edges so we can mutate targets while iterating.
        let edges: Vec<(NodeIndex, EdgeType)> = graph
            .get_dependency_edges(src)
            .into_iter()
            .filter_map(|(_, edge)| {
                edge.to
                    .filter(|_| edge.valid)
                    .map(|to| (to, edge.edge_type))
            })
            .collect();

        for (to, edge_type) in edges {
            let before = graph
                .get_node(to)
                .map(|n| (n.is_prod, n.is_dev, n.is_optional, n.is_peer));
            update_node_type_from_edge(graph, src, to, &edge_type);
            let after = graph
                .get_node(to)
                .map(|n| (n.is_prod, n.is_dev, n.is_optional, n.is_peer));

            if before != after && queued.insert(to) {
                worklist.push_back(to);
            }
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
pub async fn process_dependency<R: ManifestProvider>(
    graph: &mut DependencyGraph,
    registry: &R,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    config: &BuildDepsConfig,
) -> Result<ProcessResult, ResolveError<R::Error>> {
    // Find installation location
    match graph.find_compatible_node(node_index, &edge_info.name, &edge_info.spec) {
        FindResult::Reuse(existing_index) => {
            Ok(reuse_existing_node(graph, edge_info, existing_index))
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
                    // Importer-declared workspace: edges are fully settled by
                    // `resolve_workspace_member_edges` before the BFS starts,
                    // so reaching this arm means a declaration anomaly: an
                    // importer naming a missing member, or a workspace: spec
                    // leaked inside a published manifest (transitive deps
                    // must never declare it). Optional edges skip with a
                    // warning; anything load-bearing fails with the
                    // dependency chain attached (via `chain_err`) so the
                    // offending package is identifiable.
                    if edge_info.edge_type == EdgeType::Optional {
                        tracing::warn!(
                            "Skipping optional workspace dependency {}@{} — no matching workspace member",
                            edge_info.name,
                            edge_info.spec
                        );
                        return Ok(ProcessResult::Skipped);
                    }
                    return Err(ResolveError::Unsupported {
                        spec: edge_info.spec.clone(),
                        reason: "workspace: dependency does not match any workspace member of this project",
                    });
                }
                PackageSpec::Local {
                    protocol: Protocol::Catalog,
                    ..
                } => {
                    // catalog: specs are resolved at edge-creation time for
                    // importer (root/workspace) edges; reaching here means the
                    // catalog entry was missing — or a catalog: spec shipped
                    // inside a published manifest, which is a malformed
                    // declaration. Same policy as workspace: above.
                    if edge_info.edge_type == EdgeType::Optional {
                        tracing::warn!(
                            "Skipping optional catalog dependency {}@{} — unresolved catalog entry",
                            edge_info.name,
                            edge_info.spec
                        );
                        return Ok(ProcessResult::Skipped);
                    }
                    return Err(ResolveError::Unsupported {
                        spec: edge_info.spec.clone(),
                        reason: "catalog: spec was not resolved — published manifests must not \
                                 carry catalog:, and importer catalogs must define the entry",
                    });
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

            // Check override using resolved version (not original spec).
            // No manifest cache on this path — non-registry specs are rare.
            let resolved = match resolve_override(
                graph,
                registry,
                node_index,
                edge_info,
                &resolved.version,
                None,
            )
            .await?
            {
                Some(manifest) => ResolvedPackage {
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    manifest,
                },
                None => resolved,
            };

            Ok(place_new_node(
                graph,
                conflict_parent,
                edge_info,
                &resolved,
                config,
            ))
        }
    }
}

/// Resolve a dependency override for `edge` against the already-resolved
/// version. Single implementation shared by the demand driver and the
/// non-registry/sequential-fallback path in [`process_dependency`].
///
/// Returns `Some(manifest)` when an override rule matched and resolved, and
/// `None` when no rule applies or the override failed to resolve — npm
/// semantics fall back to the original resolution rather than erroring.
///
/// With `state`, the per-run manifest cache is consulted and populated so
/// sibling edges single-flight the override and the overridden manifest
/// persists to the project cache.
pub(crate) async fn resolve_override<R: ManifestProvider>(
    graph: &DependencyGraph,
    registry: &R,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    resolved_version: &str,
    mut state: Option<&mut ManifestState>,
) -> Result<Option<Arc<CoreVersionManifest>>, ResolveError<R::Error>> {
    let Some(override_spec) = graph.check_override(parent, &edge.name, Some(resolved_version))
    else {
        return Ok(None);
    };

    if let Some(state) = state.as_deref_mut()
        && let Some(cached) = state.get_version_manifest(&edge.name, &override_spec)
    {
        return Ok(Some(cached));
    }

    tracing::debug!(
        "Override: {}@{} (resolved {}) => {}",
        edge.name,
        edge.spec,
        resolved_version,
        override_spec
    );
    match resolve_registry_dep(registry, &edge.name, &override_spec, &edge.edge_type).await? {
        Some(overridden) => {
            if let Some(state) = state {
                state.cache_version(
                    edge.name.clone(),
                    override_spec,
                    Arc::clone(&overridden.manifest),
                );
            }
            Ok(Some(overridden.manifest))
        }
        None => Ok(None),
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
pub async fn build_deps_with_receiver<R, E: EventReceiver>(
    graph: &mut DependencyGraph,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
{
    let config = BuildDepsConfig::default().with_peer_deps(peer_deps);
    build_deps_with_config(graph, registry, config, receiver).await
}

/// Build the complete dependency tree with full configuration.
///
/// The most flexible entry point for dependency resolution: drives the
/// demand-driven BFS loop, which schedules manifest fetches on demand.
///
/// # Arguments
/// * `graph` - The dependency graph (should have root node and initial edges)
/// * `registry` - Registry client for fetching packages
/// * `config` - Build configuration (concurrency, peer_deps, ...)
/// * `receiver` - Event receiver for handling build events
///
/// # Example
/// ```ignore
/// let config = BuildDepsConfig::default().with_concurrency(50);
///
/// build_deps_with_config(&mut graph, &registry, config, &receiver).await?;
/// ```
pub async fn build_deps_with_config<R, E: EventReceiver>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: BuildDepsConfig,
    receiver: &E,
) -> Result<(), ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
{
    build_deps_with_config_output(graph, registry, config, receiver)
        .await
        .map(|_| ())
}

/// Demand-driven dependency resolution: a single BFS loop that schedules
/// manifest jobs through the [`ManifestProvider`] while owning the per-run
/// manifest cache, waiters, and in-flight de-duplication. Returns the
/// manifests resolved this run for the host to persist.
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
        "Starting demand dependency build, peer_deps: {:?}, concurrency: {}",
        config.peer_deps,
        config.concurrency
    );

    let manifest_cache = run_main_loop_bfs(graph, registry, &config, receiver).await?;

    // The BFS only builds graph structure; assign all node types in one pass now
    // that the tree is complete, so prod-ness reaches every transitive dep.
    compute_node_types(graph);

    receiver.on_event(BuildEvent::Complete {
        total_nodes: graph.graph.node_count(),
    });

    Ok(manifest_cache)
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
pub async fn resolve_with_options<R, E: EventReceiver>(
    pkg: &PackageJson,
    registry: &R,
    peer_deps: PeerDeps,
    receiver: &E,
) -> Result<PackageLock, ResolveError<R::Error>>
where
    R: ManifestProvider,
    R::Error: Send,
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

// ---- demand-resolver graph building (moved from demand::driver) ----

/// Resolve `edge` onto an already-present compatible node by marking the edge
/// resolved. Shared by the pre-fetch reuse probe ([`try_reuse_dependency`]) and
/// the post-resolution placement ([`process_dependency_with_resolved`]), whose
/// `FindResult::Reuse` arms are otherwise identical. Node types are assigned in
/// a single pass after the tree is built (see [`compute_node_types`]).
fn reuse_existing_node(
    graph: &mut DependencyGraph,
    edge: &DependencyEdgeInfo,
    existing_index: NodeIndex,
) -> ProcessResult {
    graph.mark_dependency_resolved(edge.edge_id, existing_index);
    ProcessResult::Reused(existing_index)
}

pub(crate) fn try_reuse_dependency(
    graph: &mut DependencyGraph,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
) -> Option<ProcessResult> {
    match graph.find_compatible_node(parent, &edge.name, &edge.spec) {
        FindResult::Reuse(existing_index) => Some(reuse_existing_node(graph, edge, existing_index)),
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
        FindResult::Reuse(existing_index) => reuse_existing_node(graph, edge_info, existing_index),
        FindResult::Conflict(conflict_parent) | FindResult::New(conflict_parent) => {
            place_new_node(graph, conflict_parent, edge_info, resolved, config)
        }
    }
}

/// Attach a freshly resolved package under `conflict_parent`: create the
/// node, link it physically, resolve the originating edge, and queue its own
/// dependency edges. The single placement tail shared by the spec router
/// ([`process_dependency`]) and the demand path
/// ([`process_dependency_with_resolved`]).
fn place_new_node(
    graph: &mut DependencyGraph,
    conflict_parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    resolved: &ResolvedPackage,
    config: &BuildDepsConfig,
) -> ProcessResult {
    let new_node = create_package_node(&edge.name, resolved, conflict_parent, graph);
    let new_index = graph.add_node(new_node);
    graph.add_physical_edge(conflict_parent, new_index);
    graph.mark_dependency_resolved(edge.edge_id, new_index);
    add_edges_from(
        graph,
        new_index,
        &*resolved.manifest,
        &EdgeContext::new(config.peer_deps, DevDeps::Exclude),
    );
    ProcessResult::Created(new_index)
}

pub(crate) fn chain_err<E>(
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

/// Build the graph node for an already-resolved registry manifest (override
/// resolution is applied upstream by the demand loop, which owns the per-run
/// manifest cache). Emits the resolve event and links the node.
pub(crate) fn handle_resolved_registry_manifest<E>(
    graph: &mut DependencyGraph,
    receiver: &E,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    manifest: Arc<CoreVersionManifest>,
    config: &BuildDepsConfig,
) -> ProcessResult
where
    E: EventReceiver,
{
    let resolved = ResolvedPackage {
        name: edge.name.clone(),
        version: manifest.version.clone(),
        manifest,
    };
    receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));
    process_dependency_with_resolved(graph, parent, edge, &resolved, config)
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

    /// Importer-declared workspace: edges (root → member and member → member)
    /// are settled by `resolve_workspace_member_edges` before the BFS — the
    /// build must succeed without the resolver ever seeing a workspace: spec.
    #[tokio::test]
    async fn test_workspace_member_edges_settled_before_bfs() {
        let registry = MockRegistryClient::new();

        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([(
                "lib-a".to_string(),
                "workspace:*".to_string(),
            )])),
            workspaces: Some(crate::model::package_json::WorkspacesConfig::Array(vec![
                "packages/*".to_string(),
            ])),
            ..PackageJson::new("ws-root", "1.0.0")
        };
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        let edge_ctx = EdgeContext::new(PeerDeps::Include, DevDeps::Include);
        add_edges_from(&mut graph, root_index, &root_pkg, &edge_ctx);

        // lib-b depends on lib-a via workspace:^1.0.0 — member→member edge.
        let lib_a = PackageJson::new("lib-a", "1.0.0");
        let lib_b = PackageJson {
            dependencies: Some(HashMap::from([(
                "lib-a".to_string(),
                "workspace:^1.0.0".to_string(),
            )])),
            ..PackageJson::new("lib-b", "1.0.0")
        };
        add_workspace_member(
            &mut graph,
            root_index,
            "lib-b",
            PathBuf::from("packages/lib-b"),
            &lib_b,
            &edge_ctx,
        );
        add_workspace_member(
            &mut graph,
            root_index,
            "lib-a",
            PathBuf::from("packages/lib-a"),
            &lib_a,
            &edge_ctx,
        );

        resolve_workspace_member_edges(&mut graph);

        build_deps(&mut graph, &registry, PeerDeps::Include)
            .await
            .expect("workspace edges must be settled before the BFS");

        // Every workspace: edge is resolved — none left for the BFS.
        for node in graph.graph.node_indices() {
            for (_, dep) in graph.get_dependency_edges(node) {
                assert!(
                    !dep.spec.starts_with("workspace:") || dep.valid,
                    "unresolved workspace edge survived: {}@{}",
                    dep.name,
                    dep.spec
                );
            }
        }
    }

    /// A load-bearing `workspace:` spec that resolves to no workspace member
    /// (here: a project with no workspaces at all) must fail resolution, not
    /// silently vanish.
    #[tokio::test]
    async fn test_unresolved_workspace_prod_dep_errors() {
        let registry = MockRegistryClient::new();
        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([(
                "missing-member".to_string(),
                "workspace:*".to_string(),
            )])),
            ..PackageJson::new("test-project", "1.0.0")
        };

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        add_edges_from(
            &mut graph,
            root_index,
            &root_pkg,
            &EdgeContext::new(PeerDeps::Include, DevDeps::Include),
        );

        let err = build_deps(&mut graph, &registry, PeerDeps::Include)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("workspace"),
            "expected workspace error, got: {err}"
        );
    }

    /// A `workspace:` spec inside a transitive (registry) manifest's
    /// optionalDependencies is a malformed declaration — skip it with a
    /// warning instead of failing the install.
    #[tokio::test]
    async fn test_transitive_optional_workspace_dep_skips() {
        let mut registry = MockRegistryClient::new();
        registry.add_package("dirty", "1.0.0", {
            CoreVersionManifest {
                optional_dependencies: Some(HashMap::from([(
                    "ws-leak".to_string(),
                    "workspace:*".to_string(),
                )])),
                ..create_version_manifest("dirty", "1.0.0")
            }
        });

        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([("dirty".to_string(), "^1.0.0".to_string())])),
            ..PackageJson::new("test-project", "1.0.0")
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
            .expect("optional workspace: leak must not fail the build");
        // root + dirty; the leaked workspace dep is skipped, not materialized.
        assert_eq!(graph.graph.node_count(), 2);
    }

    /// A `catalog:` spec inside a transitive (registry) manifest's regular
    /// dependencies is a malformed declaration — fail with a message naming
    /// catalog rather than the generic local-protocol error.
    #[tokio::test]
    async fn test_transitive_catalog_prod_dep_errors() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "dirty",
            "1.0.0",
            create_version_manifest_with_deps("dirty", "1.0.0", vec![("react", "catalog:")]),
        );

        let root_pkg = PackageJson {
            dependencies: Some(HashMap::from([("dirty".to_string(), "^1.0.0".to_string())])),
            ..PackageJson::new("test-project", "1.0.0")
        };

        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        add_edges_from(
            &mut graph,
            root_index,
            &root_pkg,
            &EdgeContext::new(PeerDeps::Include, DevDeps::Include),
        );

        let err = build_deps(&mut graph, &registry, PeerDeps::Include)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("catalog"),
            "expected catalog error, got: {err}"
        );
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
