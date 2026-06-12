//! The BFS driver loop. Walks the dependency graph level by level, drives the
//! fetch pipeline, and feeds resolved manifests back into the graph. Owns the
//! per-run [`ManifestState`] store and [`FetchQueues`] scheduler.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::sync::Arc;
use std::task::Poll;

use futures::stream::{FuturesUnordered, StreamExt};
use petgraph::graph::NodeIndex;

use crate::model::graph::DependencyGraph;
use crate::model::manifest::{CoreVersionManifest, NodeManifest};
use crate::model::node::EdgeType;
use crate::resolver::builder::{
    BuildDepsConfig, ProcessResult, chain_err, handle_resolved_registry_manifest,
    process_dependency, resolve_override, try_reuse_dependency,
};
use crate::resolver::edges::{DependencyEdgeInfo, collect_unresolved_edges};
use crate::resolver::registry::ResolveError;
use crate::resolver::semver::normalize_spec;
use crate::service::ManifestProvider;
use crate::spec::SpecStr;
use crate::traits::progress::{BuildEvent, EventReceiver};

use super::queue::{FetchDone, FetchKey};
use super::queue::{FetchFuture, FetchQueues};
use super::schedule::{schedule_fetch, schedule_transitive_prefetches};
use super::select::{EdgeStep, ResolutionMode, WaitKey, select_edge};
use super::state::{ManifestState, PackageVersions, ResolverManifestCache};
use crate::model::node::PeerDeps;
use crate::service::{ManifestFullData, ManifestJob, ManifestJobDone};
use crate::traits::registry::RegistryError;

/// Emit the resolution + placement events for a freshly created node.
fn emit_created_events<E: EventReceiver>(
    graph: &DependencyGraph,
    receiver: &E,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    idx: NodeIndex,
) {
    // `Created(idx)` is the index `graph.add_node` just returned, so the node
    // must exist — a miss is an invariant violation, not a normal absence.
    let node = graph
        .get_node(idx)
        .expect("a freshly created node must exist in the graph");
    receiver.on_event(BuildEvent::Resolved {
        name: &edge.name,
        version: &node.version,
    });
    // Only registry nodes carry a manifest to place; file/git/http nodes don't.
    if let NodeManifest::Registry(ref manifest) = node.manifest {
        let parent_path = graph.get_node(parent).map(|p| p.path.as_path());
        receiver.on_event(BuildEvent::PackagePlaced {
            package: manifest.as_ref().into(),
            path: &node.path,
            parent_path,
        });
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
            emit_created_events(graph, receiver, parent, edge, *idx);
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

/// Seed one BFS level's work from its nodes: the root's workspace children
/// become the next level directly, and every unresolved registry edge becomes
/// a pending edge to resolve this level.
fn seed_level<E: EventReceiver>(
    graph: &DependencyGraph,
    receiver: &E,
    current_level: &[NodeIndex],
    root_idx: NodeIndex,
) -> (Vec<NodeIndex>, VecDeque<WaitingEdge>) {
    let mut next_level = Vec::new();
    let mut level_pending = VecDeque::new();

    for node_index in current_level {
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

    (next_level, level_pending)
}

/// Run-invariant context threaded through the demand loop's per-edge helpers:
/// the manifest provider, the progress receiver, the build config, and whether
/// the registry resolves semver ranges. Bundled so the helpers stay readable.
struct DemandCtx<'a, R, E> {
    registry: &'a R,
    receiver: &'a E,
    config: &'a BuildDepsConfig,
    supports_semver: ResolutionMode,
}

impl<R, E> DemandCtx<'_, R, E>
where
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
    /// Resolve one pending edge: dispatch a non-registry spec to the legacy
    /// resolver, reuse an existing compatible node, resolve it now from a cached
    /// manifest, skip/fail it, or park it on a pending fetch and schedule it.
    async fn process_pending_edge(
        &self,
        graph: &mut DependencyGraph,
        state: &mut ManifestState,
        queues: &mut FetchQueues,
        parent: NodeIndex,
        edge: DependencyEdgeInfo,
        next_level: &mut Vec<NodeIndex>,
    ) -> Result<(), ResolveError<R::Error>> {
        self.receiver
            .on_event(BuildEvent::Resolving { name: &edge.name });

        if !edge.spec.is_registry_spec() {
            let processed = process_dependency(graph, self.registry, parent, &edge, self.config)
                .await
                .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
            handle_processed(graph, self.receiver, parent, &edge, &processed, next_level);
            return Ok(());
        }

        if let Some(processed) = try_reuse_dependency(graph, parent, &edge) {
            handle_processed(graph, self.receiver, parent, &edge, &processed, next_level);
            return Ok(());
        }

        let (real_name, real_spec) = normalize_spec(&edge.name, &edge.spec);
        let step =
            select_edge::<R::Error>(state, &edge, &real_name, &real_spec, self.supports_semver)
                .map_err(|inner| chain_err(graph, parent, &edge, inner))?;

        match step {
            EdgeStep::Resolve { manifest, alias } => {
                if let Some((alias_name, alias_spec)) = alias {
                    state.cache_version(alias_name, alias_spec, Arc::clone(&manifest));
                }
                let manifest = self
                    .apply_override(graph, state, parent, &edge, manifest)
                    .await?;
                let processed = handle_resolved_registry_manifest(
                    graph,
                    self.receiver,
                    parent,
                    &edge,
                    manifest,
                    self.config,
                );
                handle_processed(graph, self.receiver, parent, &edge, &processed, next_level);
            }
            EdgeStep::Skip => {
                self.receiver.on_event(BuildEvent::Skipped {
                    name: &edge.name,
                    spec: &edge.spec,
                });
            }
            EdgeStep::Fail(message) => {
                if edge.edge_type == EdgeType::Optional {
                    self.receiver.on_event(BuildEvent::Skipped {
                        name: &edge.name,
                        spec: &edge.spec,
                    });
                } else {
                    return Err(chain_err(graph, parent, &edge, registry_error(message)));
                }
            }
            EdgeStep::Park { wait, fetch } => {
                match wait {
                    WaitKey::Full(key) => state.park_on_package(key, (parent, edge)),
                    WaitKey::Version(key) => state.park_on_version(key, (parent, edge)),
                }
                schedule_fetch(state, queues, fetch, self.supports_semver);
            }
        }

        Ok(())
    }

    /// Apply a dependency override for `edge`, returning the manifest to install.
    ///
    /// The override is routed through the per-run manifest cache: a sibling
    /// edge's already-resolved override is reused (single-flight within the
    /// run), and a freshly-resolved one is recorded so it persists to the
    /// project cache. Without this, every edge sharing an override re-fetches
    /// it and the overridden manifest is omitted from the warm cache, forcing a
    /// network round-trip on every subsequent install. Falls back to the
    /// original manifest when no override applies or the override fails to
    /// resolve.
    async fn apply_override(
        &self,
        graph: &mut DependencyGraph,
        state: &mut ManifestState,
        parent: NodeIndex,
        edge: &DependencyEdgeInfo,
        manifest: Arc<CoreVersionManifest>,
    ) -> Result<Arc<CoreVersionManifest>, ResolveError<R::Error>> {
        match resolve_override(
            graph,
            self.registry,
            parent,
            edge,
            &manifest.version,
            Some(state),
        )
        .await
        .map_err(|inner| chain_err(graph, parent, edge, inner))?
        {
            Some(overridden) => Ok(overridden),
            None => Ok(manifest),
        }
    }

    /// Last-resort path when the fetch stream drains while edges are still parked:
    /// resolve every remaining waiter sequentially through the legacy resolver.
    async fn resolve_waiters_sequentially(
        &self,
        graph: &mut DependencyGraph,
        state: &mut ManifestState,
        next_level: &mut Vec<NodeIndex>,
    ) -> Result<(), ResolveError<R::Error>> {
        for (parent, edge) in state.drain_waiters() {
            let processed = process_dependency(graph, self.registry, parent, &edge, self.config)
                .await
                .map_err(|inner| chain_err(graph, parent, &edge, inner))?;
            handle_processed(graph, self.receiver, parent, &edge, &processed, next_level);
        }
        Ok(())
    }

    /// Top up the in-flight fetch set from the queue up to `concurrency`, spawning
    /// each as a provider job so independent fetch + parse run in parallel.
    fn pump_fetches(
        &self,
        fetches: &mut FuturesUnordered<FetchFuture>,
        fetch_queues: &mut FetchQueues,
        concurrency: usize,
    ) {
        while fetches.len() < concurrency {
            let Some(request) = fetch_queues.pop() else {
                break;
            };
            fetches.push(fetch_registry_manifest(self.registry.clone(), request));
        }
    }
}

/// Demand-driven BFS resolution loop.
///
/// Walks the dependency graph level by level. Within a level it schedules
/// manifest fetches as provider jobs on a `FuturesUnordered`, then drains them
/// as they complete and feeds resolved versions back into the graph so the next
/// level can be discovered. [`ManifestState`] owns the per-run manifest cache,
/// waiters, and inflight de-duplication; [`FetchQueues`] prioritises on-demand
/// fetches over speculative prefetches. Returns the warmed manifest cache so the
/// caller can reuse it.
pub(crate) async fn run_main_loop_bfs<R, E>(
    graph: &mut DependencyGraph,
    registry: &R,
    config: &BuildDepsConfig,
    receiver: &E,
) -> Result<ResolverManifestCache, ResolveError<R::Error>>
where
    // `R` is the I/O boundary (a manifest provider); its error must be `Send`
    // because fetch jobs are polled on a `FuturesUnordered`. `E` receives
    // progress events as the loop advances.
    R: ManifestProvider,
    R::Error: Send,
    E: EventReceiver,
{
    let supports_semver =
        ResolutionMode::from_supports_semver(registry.supports_semver_resolution());
    let concurrency = config.concurrency.max(1);
    let ctx = DemandCtx {
        registry,
        receiver,
        config,
        supports_semver,
    };

    let mut state = ManifestState::seeded(
        config
            .project_cache
            .as_ref()
            .map(|pc| pc.resolved_manifests())
            .unwrap_or_default(),
    );
    let mut queues = FetchQueues::default();
    let mut fetches: FuturesUnordered<FetchFuture> = FuturesUnordered::new();

    let root_idx = graph.root_index;
    let mut current_level = vec![root_idx];

    // Resolve the graph one BFS level at a time; each iteration discovers the
    // next level from the edges it resolves.
    while !current_level.is_empty() {
        receiver.on_event(BuildEvent::LevelStart {
            node_count: current_level.len(),
        });

        let (mut next_level, mut level_pending) =
            seed_level(graph, receiver, &current_level, root_idx);

        // Drain loop: keep the fetch pipeline saturated and process edges as
        // their manifests arrive, until this level has nothing left in flight.
        loop {
            ctx.pump_fetches(&mut fetches, &mut queues, concurrency);

            while let Some((parent, edge)) = level_pending.pop_front() {
                ctx.process_pending_edge(
                    graph,
                    &mut state,
                    &mut queues,
                    parent,
                    edge,
                    &mut next_level,
                )
                .await?;
                ctx.pump_fetches(&mut fetches, &mut queues, concurrency);
            }

            loop {
                let ready = poll_fn(|cx| match fetches.poll_next_unpin(cx) {
                    Poll::Ready(done) => Poll::Ready(done),
                    Poll::Pending => Poll::Ready(None),
                })
                .await;
                let Some(done) = ready else {
                    break;
                };
                let done = done.map_err(|e| {
                    registry_error::<R::Error>(format!("manifest fetch task failed: {e}"))
                })?;

                apply_fetch_result(
                    &mut state,
                    &mut queues,
                    done,
                    supports_semver,
                    config.peer_deps,
                    &mut level_pending,
                );
            }

            if !level_pending.is_empty() {
                continue;
            }

            if state.has_pending_waiters() {
                ctx.pump_fetches(&mut fetches, &mut queues, concurrency);
            } else {
                break;
            }

            let Some(done) = fetches.next().await else {
                let (full_waiters, version_waiters) = state.pending_waiter_counts();
                tracing::warn!(
                    full_waiters,
                    version_waiters,
                    "manifest fetch stream ended with pending resolver waiters; falling back to sequential resolution"
                );
                ctx.resolve_waiters_sequentially(graph, &mut state, &mut next_level)
                    .await?;
                break;
            };
            let done = done.map_err(|e| {
                registry_error::<R::Error>(format!("manifest fetch task failed: {e}"))
            })?;

            apply_fetch_result(
                &mut state,
                &mut queues,
                done,
                supports_semver,
                config.peer_deps,
                &mut level_pending,
            );
        }

        receiver.on_event(BuildEvent::LevelComplete {
            next_level_count: next_level.len(),
        });
        current_level = next_level;
    }

    Ok(state.into_resolver_cache())
}

// ---- Orchestration: state transitions over the store + queue ----
//
// These tie the manifest store and the fetch scheduler together; the store and
// the queue stay unaware of each other.

/// A parked edge waiting on a pending fetch (matches `state`'s waiter payload).
pub(super) type WaitingEdge = (NodeIndex, DependencyEdgeInfo);

pub(super) fn registry_error<E: From<RegistryError>>(
    message: impl Into<String>,
) -> ResolveError<E> {
    ResolveError::Registry(RegistryError(anyhow::anyhow!(message.into())).into())
}

async fn fetch_registry_manifest_inner<R: ManifestProvider>(
    registry: R,
    request: ManifestJob,
) -> FetchDone {
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

/// Create a fetch job. Native runs it on the multi-threaded runtime so
/// independent fetch + parse jobs progress in parallel; wasm has no Tokio
/// `LocalSet`, so it stays as a local future driven by `FuturesUnordered`.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_registry_manifest<R: ManifestProvider>(registry: R, request: ManifestJob) -> FetchFuture
where
    R::Error: Send,
{
    Box::pin(async move {
        tokio::spawn(fetch_registry_manifest_inner(registry, request))
            .await
            .map_err(|error| {
                if error.is_panic() {
                    std::panic::resume_unwind(error.into_panic());
                }
                error.to_string()
            })
    })
}

#[cfg(target_arch = "wasm32")]
fn fetch_registry_manifest<R: ManifestProvider>(registry: R, request: ManifestJob) -> FetchFuture {
    Box::pin(async move { Ok(fetch_registry_manifest_inner(registry, request).await) })
}

/// Apply one completed fetch to the store: cache it, wake parked edges, and
/// schedule any transitive prefetches.
pub(super) fn apply_fetch_result(
    state: &mut ManifestState,
    queues: &mut FetchQueues,
    done: FetchDone,
    supports_semver: ResolutionMode,
    peer_deps: PeerDeps,
    ready: &mut VecDeque<WaitingEdge>,
) {
    let done_key = done.key();
    queues.complete(&done_key);

    match done {
        FetchDone::Full { name, result } => {
            match result {
                Ok(ManifestFullData::Full {
                    manifest: full,
                    speculative,
                }) => {
                    if let Some((resolved_spec, manifest)) = speculative {
                        state.cache_version(name.clone(), resolved_spec, Arc::clone(&manifest));
                        schedule_transitive_prefetches(
                            state,
                            queues,
                            &manifest,
                            peer_deps,
                            supports_semver,
                        );
                    }
                    state.set_package(name.clone(), PackageVersions::Full(full));
                }
                Ok(ManifestFullData::Versions(versions)) => {
                    state.set_package(name.clone(), PackageVersions::List(versions));
                }
                Err(e) => {
                    state.set_package(name.clone(), PackageVersions::Failed(e));
                }
            }
            state.wake_package(&name, ready);
        }
        FetchDone::Version { name, spec, result } => {
            match result {
                Ok(manifest) => {
                    state.cache_version(name.clone(), spec.clone(), Arc::clone(&manifest));
                    schedule_transitive_prefetches(
                        state,
                        queues,
                        &manifest,
                        peer_deps,
                        supports_semver,
                    );
                }
                Err(e) => state.fail_version(&name, &spec, e),
            }
            state.wake_version(&name, &spec, ready);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use petgraph::graph::EdgeIndex;

    use super::*;
    use crate::model::manifest::CoreVersionManifest;
    use crate::model::package_json::PackageJson;
    use crate::resolver::builder::resolve;
    use crate::service::{ManifestJob, ManifestJobDone};
    use crate::traits::registry::mock::{MockError, MockRegistryClient};

    /// A parked waiter; node/edge indices are placeholders — `apply_fetch_result`
    /// only routes the payload, it never dereferences the graph.
    fn test_edge(name: &str, spec: &str) -> WaitingEdge {
        (
            NodeIndex::new(0),
            DependencyEdgeInfo {
                edge_id: EdgeIndex::new(0),
                name: name.to_string(),
                spec: spec.to_string(),
                edge_type: EdgeType::Prod,
            },
        )
    }

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

    #[derive(Clone)]
    struct CountingRegistry {
        inner: MockRegistryClient,
        shared_version_jobs: Arc<AtomicUsize>,
    }

    impl crate::traits::registry::RegistryClient for CountingRegistry {
        type Error = MockError;
    }

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
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

    #[tokio::test]
    async fn prod_flip_propagates_through_already_resolved_subtree() {
        // root.devDeps: shared (shallow dev path) -> leaf
        // root.deps: prod1 -> prod2 -> prod3 -> shared (deep prod path)
        // `shared` is created dev first; its edge shared->leaf is resolved while
        // shared is still dev (=> leaf dev). The deep prod path later flips
        // `shared` to prod; the finalize fixpoint must re-propagate prod-ness so
        // `leaf` (shared's prod child) is no longer marked dev, matching npm.
        let mut inner = MockRegistryClient::new();
        inner.add_package(
            "prod1",
            "1.0.0",
            create_version_manifest_with_deps("prod1", "1.0.0", vec![("prod2", "^1.0.0")]),
        );
        inner.add_package(
            "prod2",
            "1.0.0",
            create_version_manifest_with_deps("prod2", "1.0.0", vec![("prod3", "^1.0.0")]),
        );
        inner.add_package(
            "prod3",
            "1.0.0",
            create_version_manifest_with_deps("prod3", "1.0.0", vec![("shared", "^1.0.0")]),
        );
        inner.add_package(
            "shared",
            "1.0.0",
            create_version_manifest_with_deps("shared", "1.0.0", vec![("leaf", "^1.0.0")]),
        );
        inner.add_package("leaf", "1.0.0", create_version_manifest("leaf", "1.0.0"));

        let registry = inner;
        let pkg = PackageJson {
            dependencies: Some(HashMap::from([("prod1".to_string(), "1.0.0".to_string())])),
            dev_dependencies: Some(HashMap::from([("shared".to_string(), "1.0.0".to_string())])),
            ..PackageJson::new("test-project", "1.0.0")
        };

        let lock = resolve(&pkg, &registry).await.unwrap();
        let shared = lock
            .packages
            .get("node_modules/shared")
            .expect("shared present");
        let leaf = lock
            .packages
            .get("node_modules/leaf")
            .expect("leaf present");
        assert_ne!(
            shared.dev,
            Some(true),
            "shared reached via prod, must not be dev"
        );
        assert_ne!(
            leaf.dev,
            Some(true),
            "leaf is prod child of prod shared, must not be dev"
        );
    }

    #[tokio::test]
    async fn workspace_prod_chain_overrides_root_dev_mark() {
        // root.devDeps: shared (shallow dev via the root importer => shared.is_dev),
        //               rdev   (pure dev leaf, sanity check)
        // workspace `app`.deps: mid -> deep -> shared (deep prod via a workspace)
        // `shared` is reachable through a workspace's prod chain, so it must be
        // prod, not dev — the cross-workspace analog of the resolve-order bug.
        use crate::model::graph::DependencyGraph;
        use crate::model::node::DevDeps;
        use crate::resolver::builder::{
            BuildDepsConfig, add_edges_from, add_workspace_member, build_deps_with_config_output,
        };
        use crate::resolver::edges::EdgeContext;
        use crate::traits::progress::NoopReceiver;
        use std::path::PathBuf;

        let mut inner = MockRegistryClient::new();
        inner.add_package(
            "mid",
            "1.0.0",
            create_version_manifest_with_deps("mid", "1.0.0", vec![("deep", "^1.0.0")]),
        );
        inner.add_package(
            "deep",
            "1.0.0",
            create_version_manifest_with_deps("deep", "1.0.0", vec![("shared", "^1.0.0")]),
        );
        inner.add_package(
            "shared",
            "1.0.0",
            create_version_manifest("shared", "1.0.0"),
        );
        inner.add_package("rdev", "1.0.0", create_version_manifest("rdev", "1.0.0"));
        let registry = inner;

        let root_pkg = PackageJson {
            dev_dependencies: Some(HashMap::from([
                ("shared".to_string(), "^1.0.0".to_string()),
                ("rdev".to_string(), "^1.0.0".to_string()),
            ])),
            ..PackageJson::new("root", "1.0.0")
        };
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg.clone());
        let root_index = graph.root_index;
        let edge_ctx = EdgeContext::new(PeerDeps::Skip, DevDeps::Include);
        add_edges_from(&mut graph, root_index, &root_pkg, &edge_ctx);

        let app = PackageJson {
            dependencies: Some(HashMap::from([("mid".to_string(), "^1.0.0".to_string())])),
            ..PackageJson::new("app", "1.0.0")
        };
        add_workspace_member(
            &mut graph,
            root_index,
            "app",
            PathBuf::from("packages/app"),
            &app,
            &edge_ctx,
        );

        let config = BuildDepsConfig::default().with_peer_deps(PeerDeps::Skip);
        build_deps_with_config_output(&mut graph, &registry, config, &NoopReceiver)
            .await
            .unwrap();
        let (packages, _) = graph.serialize_to_packages(&PathBuf::from("."));

        let dev_of = |k: &str| packages.get(k).unwrap_or_else(|| panic!("{k} present")).dev;
        // workspace prod subtree is prod
        assert_ne!(dev_of("node_modules/mid"), Some(true), "workspace prod dep");
        assert_ne!(
            dev_of("node_modules/deep"),
            Some(true),
            "workspace prod transitive"
        );
        // shared: dev via root, but prod via the workspace chain => prod wins
        assert_ne!(
            dev_of("node_modules/shared"),
            Some(true),
            "shared reached via workspace prod chain must not be dev"
        );
        // sanity: a pure root devDep leaf is still dev
        assert_eq!(
            dev_of("node_modules/rdev"),
            Some(true),
            "pure dev leaf stays dev"
        );
    }

    #[test]
    fn apply_version_success_caches_and_wakes_waiter() {
        let mut state = ManifestState::seeded(Vec::new());
        let mut queues = FetchQueues::default();
        let mut ready = VecDeque::new();
        state.park_on_version(
            ("shared".to_string(), "^1.0.0".to_string()),
            test_edge("shared", "^1.0.0"),
        );

        apply_fetch_result(
            &mut state,
            &mut queues,
            FetchDone::Version {
                name: "shared".to_string(),
                spec: "^1.0.0".to_string(),
                result: Ok(Arc::new(create_version_manifest("shared", "1.2.3"))),
            },
            ResolutionMode::Semver,
            PeerDeps::Skip,
            &mut ready,
        );

        // Manifest cached under the requested spec, and the parked edge is woken.
        assert!(state.get_version_manifest("shared", "^1.0.0").is_some());
        assert_eq!(ready.len(), 1);
    }

    #[test]
    fn apply_version_failure_records_and_wakes_waiter() {
        let mut state = ManifestState::seeded(Vec::new());
        let mut queues = FetchQueues::default();
        let mut ready = VecDeque::new();
        state.park_on_version(
            ("shared".to_string(), "^1.0.0".to_string()),
            test_edge("shared", "^1.0.0"),
        );

        apply_fetch_result(
            &mut state,
            &mut queues,
            FetchDone::Version {
                name: "shared".to_string(),
                spec: "^1.0.0".to_string(),
                result: Err("registry exploded".to_string()),
            },
            ResolutionMode::Semver,
            PeerDeps::Skip,
            &mut ready,
        );

        // Failure recorded so the woken edge resolves to skip-or-error, not a hang.
        assert_eq!(
            state.get_version_failure("shared", "^1.0.0"),
            Some("registry exploded")
        );
        assert_eq!(ready.len(), 1);
    }

    #[test]
    fn apply_version_success_schedules_transitive_prefetch() {
        let mut state = ManifestState::seeded(Vec::new());
        let mut queues = FetchQueues::default();
        let mut ready = VecDeque::new();

        apply_fetch_result(
            &mut state,
            &mut queues,
            FetchDone::Version {
                name: "a".to_string(),
                spec: "^1.0.0".to_string(),
                result: Ok(Arc::new(create_version_manifest_with_deps(
                    "a",
                    "1.0.0",
                    vec![("dep", "^2.0.0")],
                ))),
            },
            ResolutionMode::Semver,
            PeerDeps::Skip,
            &mut ready,
        );

        // The resolved manifest's registry dep is queued as a speculative prefetch.
        let job = queues
            .pop()
            .expect("transitive prefetch for `dep` should be queued");
        assert_eq!(
            job.key(),
            FetchKey::Version("dep".to_string(), "^2.0.0".to_string())
        );
    }
}
