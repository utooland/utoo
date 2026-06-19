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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use petgraph::graph::{EdgeIndex, NodeIndex};

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::manifest::CoreVersionManifest;
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;
use crate::model::package_lock::PackageLock;
use crate::resolver::demand::{ManifestState, run_main_loop_bfs};
use crate::resolver::registry::{ResolveError, resolve_registry_dep};
use crate::service::ManifestProvider;
use crate::spec::{Catalogs, PackageSpec, Protocol};
use crate::traits::progress::{BuildEvent, EventReceiver, NoopReceiver};
use crate::traits::registry::ResolvedPackage;

/// Dispatch a git/github spec to the system-`git`-backed resolver when the
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
/// BFS only reads the tarball's manifest; the content is materialized into
/// `node_modules` at install time, never the global cache. See
/// [`super::http`] module docs.
async fn resolve_http_dep(
    url: &str,
    fetch_cache: &HttpFetchCache,
) -> anyhow::Result<ResolvedPackage> {
    #[cfg(feature = "http-tarball")]
    {
        crate::resolver::http::resolve_http_dep(url, fetch_cache).await
    }
    #[cfg(not(feature = "http-tarball"))]
    {
        let _ = fetch_cache;
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
// Node-type propagation and graph placement live in their own passes;
// re-exported here for the callers that historically reached them through
// builder.
#[cfg(feature = "http-tarball")]
use super::file::process_file_dep;
pub use super::node_types::{compute_node_types, update_node_type_from_edge};
pub use super::placement::process_dependency_with_resolved;
pub(crate) use super::placement::{
    chain_err, handle_resolved_registry_manifest, place_new_node, reuse_existing_node,
    try_reuse_dependency,
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
/// Shared optional-edge policy for the non-registry resolvers (git/http): a
/// failed optional edge logs and resolves to `Skipped`; a load-bearing one
/// returns the spec-specific error built by `wrap`.
/// Collapse a non-registry resolver's result: success → `Some`; an *optional*
/// edge's failure → `None` (caller skips / falls back); a load-bearing failure
/// → the wrapped cause. Lets [`resolve_remote_spec`] keep each protocol arm to a
/// single expression.
fn resolve_or_skip<E>(
    result: anyhow::Result<ResolvedPackage>,
    edge_type: &EdgeType,
    name: &str,
    spec: &str,
    kind: &str,
    wrap: impl FnOnce(anyhow::Error) -> ResolveError<E>,
) -> Result<Option<ResolvedPackage>, ResolveError<E>> {
    match result {
        Ok(pkg) => Ok(Some(pkg)),
        Err(e) if *edge_type == EdgeType::Optional => {
            tracing::debug!("Skipping optional {kind} dependency {name}@{spec}: {e}");
            Ok(None)
        }
        Err(e) => Err(wrap(e)),
    }
}

/// Shared policy for `workspace:`/`catalog:` specs that reach the BFS — a
/// declaration anomaly (see the call sites for why). Optional edges skip with
/// a warning; load-bearing ones fail as `Unsupported`.
fn skip_optional_or_unsupported<E>(
    edge: &DependencyEdgeInfo,
    kind: &str,
    warn_detail: &str,
    reason: &'static str,
) -> Result<ProcessResult, ResolveError<E>> {
    if edge.edge_type == EdgeType::Optional {
        tracing::warn!(
            "Skipping optional {kind} dependency {}@{} — {warn_detail}",
            edge.name,
            edge.spec
        );
        return Ok(ProcessResult::Skipped);
    }
    Err(ResolveError::Unsupported {
        spec: edge.spec.clone(),
        reason,
    })
}

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
                    return skip_optional_or_unsupported(
                        edge_info,
                        "workspace",
                        "no matching workspace member",
                        "workspace: dependency does not match any workspace member of this project",
                    );
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
                    return skip_optional_or_unsupported(
                        edge_info,
                        "catalog",
                        "unresolved catalog entry",
                        "catalog: spec was not resolved — published manifests must not \
                         carry catalog:, and importer catalogs must define the entry",
                    );
                }
                PackageSpec::Local {
                    protocol: Protocol::File,
                    path,
                } => {
                    #[cfg(feature = "http-tarball")]
                    {
                        match process_file_dep(graph, node_index, conflict_parent, edge_info, path)
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
                // All remote protocols share one resolver — see
                // [`resolve_remote_spec`]. The exhaustive listing (no `_`) keeps
                // a new PackageSpec variant a compile error rather than silently
                // routing it to the wrong resolver.
                PackageSpec::Registry { .. }
                | PackageSpec::Http { .. }
                | PackageSpec::Git { .. }
                | PackageSpec::GitHub { .. } => {
                    match resolve_remote_spec(
                        registry,
                        &parsed_spec,
                        &edge_info.spec,
                        &edge_info.name,
                        &edge_info.edge_type,
                        config,
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

            // Apply any override against the resolved version (not the original
            // spec). Non-registry override targets resolve here too.
            // No manifest cache on this path — non-registry specs are rare.
            let resolved = match resolve_override(
                graph,
                registry,
                node_index,
                edge_info,
                &resolved.version,
                config,
                None,
            )
            .await?
            {
                Some(manifest) => ResolvedPackage::from_manifest(manifest.name.clone(), manifest),
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

/// Resolve a registry / git / http(s) spec to a package, mirroring the protocol
/// dispatch in [`process_dependency`]. `None` means an *optional* edge's
/// resolution failed and the caller should skip / fall back. Shared by the
/// normal dependency path and override resolution so both treat these protocols
/// identically.
///
/// `file:` is intentionally excluded: its base dir (the edge's parent for a
/// normal dep, the root project for an override) and its dir→Link placement
/// differ between the two callers, so each routes `file:` through
/// [`process_file_dep`] with its own `node_index`.
async fn resolve_remote_spec<R: ManifestProvider>(
    registry: &R,
    parsed: &PackageSpec,
    raw_spec: &str,
    name: &str,
    edge_type: &EdgeType,
    config: &BuildDepsConfig,
) -> Result<Option<ResolvedPackage>, ResolveError<R::Error>> {
    match parsed {
        // The registry path keeps receiving the raw spec so `npm:` alias /
        // dist-tag / range handling is identical regardless of caller.
        PackageSpec::Registry { .. } => {
            resolve_registry_dep(registry, name, raw_spec, edge_type).await
        }
        PackageSpec::Http { url } => resolve_or_skip(
            resolve_http_dep(url, &config.http_fetch_cache).await,
            edge_type,
            name,
            raw_spec,
            "HTTP",
            |source| ResolveError::Http {
                url: url.clone(),
                source,
            },
        ),
        PackageSpec::Git { .. } | PackageSpec::GitHub { .. } => resolve_or_skip(
            resolve_git_dep(
                config.cache_dir.as_deref(),
                parsed,
                name,
                &config.git_clone_cache,
            )
            .await,
            edge_type,
            name,
            raw_spec,
            "git",
            |source| ResolveError::Git {
                url: raw_spec.to_string(),
                source,
            },
        ),
        // Callers route file:/link:/workspace:/catalog: elsewhere; reaching here
        // is an internal routing bug, not a user error.
        PackageSpec::Local { .. } => Err(ResolveError::Unsupported {
            spec: raw_spec.to_string(),
            reason: "internal: resolve_remote_spec received a non-remote spec",
        }),
    }
}

/// Resolve a dependency override for `edge` against the already-resolved
/// version, routing the override *target* by protocol like a normal dependency
/// spec (registry range / `file:` tarball / http(s) tarball / git — not just a
/// registry version, which used to turn `file:x.tgz` overrides into bogus 404s).
/// Shared by the demand driver and [`process_dependency`].
///
/// Always yields a *manifest* (or `None` to fall back to the original), so the
/// callers place it like any registry node and stay agnostic of the protocol.
/// `file:` *directory* overrides would need a self-placed Link node the manifest
/// contract can't carry, so they are an explicit error. With `state`, the
/// per-run manifest cache single-flights the override across sibling edges.
pub(crate) async fn resolve_override<R: ManifestProvider>(
    graph: &DependencyGraph,
    registry: &R,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    resolved_version: &str,
    config: &BuildDepsConfig,
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

    let parsed = PackageSpec::from(override_spec.as_str());
    let resolved = match &parsed {
        // `file:` overrides resolve relative to the ROOT project (overrides are
        // root-declared; a transitive parent is usually a registry node with no
        // `file:` base).
        PackageSpec::Local {
            protocol: Protocol::File,
            path,
        } => {
            resolve_override_file_tarball::<R::Error>(graph, &edge.edge_type, &override_spec, path)
                .await?
        }
        PackageSpec::Local { .. } => {
            return Err(ResolveError::Unsupported {
                spec: override_spec,
                reason: "link:/portal:/workspace: overrides are not supported",
            });
        }
        // registry / http / git share the normal-dependency resolver.
        _ => resolve_remote_spec(
            registry,
            &parsed,
            &override_spec,
            &edge.name,
            &edge.edge_type,
            config,
        )
        .await?
        .map(|pkg| pkg.manifest),
    };

    // Optional edge whose override failed to resolve → fall back to the original.
    let Some(manifest) = resolved else {
        return Ok(None);
    };
    if let Some(state) = state {
        state.cache_version(edge.name.clone(), override_spec, Arc::clone(&manifest));
    }
    Ok(Some(manifest))
}

/// Resolve a `file:` override target to a manifest, relative to the root project
/// (overrides are root-declared). A tarball → manifest pinned to `file:<abs>`; a
/// directory → error (it would need a Link node the manifest contract can't
/// carry). Read-only, so the resolver stays decoupled from placement.
async fn resolve_override_file_tarball<E>(
    graph: &DependencyGraph,
    edge_type: &EdgeType,
    override_spec: &str,
    path: &str,
) -> Result<Option<Arc<CoreVersionManifest>>, ResolveError<E>> {
    #[cfg(not(feature = "http-tarball"))]
    {
        let _ = (graph, edge_type, path);
        Err(ResolveError::Unsupported {
            spec: override_spec.to_string(),
            reason: "file: overrides require the 'http-tarball' feature",
        })
    }
    #[cfg(feature = "http-tarball")]
    {
        let file_err = |source| ResolveError::File {
            spec: override_spec.to_string(),
            source,
        };
        // The root node is a construction-time invariant; resolve the override's
        // path against it (overrides are root-declared).
        let abs = graph
            .get_node(graph.root_index)
            .expect("root node is always present")
            .path
            .join(path);
        match std::fs::metadata(&abs) {
            Ok(m) if m.is_dir() => Err(ResolveError::Unsupported {
                spec: override_spec.to_string(),
                reason: "file: directory overrides (symlink) are not supported — use a tarball (.tgz)",
            }),
            // Same local-tarball read+parse the normal file-dep resolver uses.
            Ok(_) => match crate::resolver::tar::read_local_tarball_manifest(abs).await {
                Ok(m) => Ok(Some(Arc::new(m))),
                Err(_) if *edge_type == EdgeType::Optional => Ok(None),
                Err(source) => Err(file_err(source)),
            },
            Err(_) if *edge_type == EdgeType::Optional => Ok(None),
            Err(e) => Err(file_err(
                anyhow::Error::new(e).context(format!("file: override target {}", abs.display())),
            )),
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
/// Demand-driven dependency resolution: a single BFS loop that schedules
/// manifest jobs through the [`ManifestProvider`] while owning the per-run
/// manifest cache, waiters, and in-flight de-duplication.
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
    tracing::debug!(
        "Starting demand dependency build, peer_deps: {:?}, concurrency: {}",
        config.peer_deps,
        config.concurrency
    );

    run_main_loop_bfs(graph, registry, &config, receiver).await?;

    // The BFS only builds graph structure; assign all node types in one pass now
    // that the tree is complete, so prod-ness reaches every transitive dep.
    compute_node_types(graph);

    receiver.on_event(BuildEvent::Complete {
        total_nodes: graph.graph.node_count(),
    });

    Ok(())
}

// ============================================================================
// High-level API
// ============================================================================

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
    let (packages, _total) = graph.serialize_to_packages(Path::new("."));
    Ok(PackageLock::new(&pkg.name, &pkg.version, packages))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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
