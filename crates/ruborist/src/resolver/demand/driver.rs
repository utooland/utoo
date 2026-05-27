//! The BFS driver loop. Walks the dependency graph level by level, drives the
//! fetch pipeline, and feeds resolved manifests back into the graph. Owns the
//! per-run [`ManifestState`] store and [`FetchQueues`] scheduler.

use std::collections::VecDeque;
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use petgraph::graph::NodeIndex;

use crate::model::graph::DependencyGraph;
use crate::model::manifest::NodeManifest;
use crate::model::node::EdgeType;
use crate::resolver::builder::{
    BuildDepsConfig, ProcessResult, chain_err, handle_resolved_registry_manifest,
    process_dependency, try_reuse_dependency,
};
use crate::resolver::edges::{DependencyEdgeInfo, collect_unresolved_edges};
use crate::resolver::registry::ResolveError;
use crate::resolver::semver::normalize_spec;
use crate::service::ManifestProvider;
use crate::spec::SpecStr;
use crate::traits::progress::{BuildEvent, EventReceiver};

use super::queue::{FetchDone, FetchKey};
use super::queue::{FetchFuture, FetchPriority, FetchQueues};
use super::select::{EdgeStep, FetchPlan, ResolutionMode, WaitKey, select_edge};
use super::state::{ManifestState, ResolverManifestCache};
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::{DevDeps, PeerDeps};
use crate::resolver::edges::DependencySource;
use crate::service::{ManifestFullData, ManifestJob, ManifestJobDone, MetadataFormat};
use crate::traits::registry::RegistryError;

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

        let mut next_level = Vec::new();
        let mut level_pending = VecDeque::new();

        // Seed this level's work queue: workspace children of the root plus
        // every unresolved registry edge on the level's nodes.
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

        // Drain loop: keep the fetch pipeline saturated and process edges as
        // their manifests arrive, until this level has nothing left in flight.
        loop {
            pump_fetches(&mut fetches, &mut queues, registry, concurrency);

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
                let step =
                    select_edge::<R::Error>(&state, &edge, &real_name, &real_spec, supports_semver)
                        .map_err(|inner| chain_err(graph, parent, &edge, inner))?;

                match step {
                    EdgeStep::Resolve { manifest, alias } => {
                        if let Some(key) = alias {
                            state.version.cache.insert(key, Arc::clone(&manifest));
                        }
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
                    }
                    EdgeStep::Skip => {
                        receiver.on_event(BuildEvent::Skipped {
                            name: &edge.name,
                            spec: &edge.spec,
                        });
                    }
                    EdgeStep::Fail(message) => {
                        if edge.edge_type == EdgeType::Optional {
                            receiver.on_event(BuildEvent::Skipped {
                                name: &edge.name,
                                spec: &edge.spec,
                            });
                        } else {
                            return Err(chain_err(graph, parent, &edge, registry_error(message)));
                        }
                    }
                    EdgeStep::Park { wait, fetch } => {
                        match wait {
                            WaitKey::Full(key) => {
                                state
                                    .full
                                    .waiters
                                    .entry(key)
                                    .or_default()
                                    .push((parent, edge));
                            }
                            WaitKey::Version(key) => {
                                state
                                    .version
                                    .waiters
                                    .entry(key)
                                    .or_default()
                                    .push((parent, edge));
                            }
                        }
                        match fetch {
                            FetchPlan::Registry { name, spec } => schedule_registry_fetch(
                                &mut state,
                                &mut queues,
                                name,
                                spec,
                                supports_semver,
                                FetchPriority::Demand,
                            ),
                            FetchPlan::Extract {
                                name,
                                version,
                                full,
                            } => enqueue_version_extract(&mut queues, name, version, full),
                            FetchPlan::VersionFetch { name, version } => {
                                enqueue_version_fetch(&mut queues, name, version, supports_semver)
                            }
                        }
                    }
                }

                pump_fetches(&mut fetches, &mut queues, registry, concurrency);
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

            if !state.full.waiters.is_empty() || !state.version.waiters.is_empty() {
                pump_fetches(&mut fetches, &mut queues, registry, concurrency);
            }

            if state.full.waiters.is_empty() && state.version.waiters.is_empty() {
                break;
            }

            let Some(done) = fetches.next().await else {
                tracing::warn!(
                    full_waiters = state.full.waiters.values().map(Vec::len).sum::<usize>(),
                    version_waiters = state.version.waiters.values().map(Vec::len).sum::<usize>(),
                    "manifest fetch stream ended with pending resolver waiters; falling back to sequential resolution"
                );
                let mut fallback = Vec::new();
                for (_, waiters) in state.full.waiters.drain() {
                    fallback.extend(waiters);
                }
                for (_, waiters) in state.version.waiters.drain() {
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

pub(super) fn registry_error<E>(message: impl Into<String>) -> ResolveError<E>
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

/// Spawn a fetch job. Native runs it on the multi-threaded runtime so
/// independent fetch + parse jobs progress in parallel; wasm has no threads, so
/// it runs on the current local set.
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

pub(super) fn pump_fetches<R>(
    fetches: &mut FuturesUnordered<FetchFuture>,
    fetch_queues: &mut FetchQueues,
    registry: &R,
    concurrency: usize,
) where
    R: ManifestProvider,
    R::Error: Send,
{
    while fetches.len() < concurrency {
        let Some(request) = fetch_queues.pop() else {
            break;
        };
        fetches.push(fetch_registry_manifest(registry.clone(), request));
    }
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
                    state.full.cache.insert(name.clone(), full);
                }
                Ok(ManifestFullData::Versions(versions)) => {
                    state.versions_cache.insert(name.clone(), versions);
                }
                Err(e) => {
                    state.full.failures.insert(name.clone(), e);
                }
            }
            state.full.wake(&name, ready);
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

fn version_metadata_format(supports_semver: ResolutionMode) -> MetadataFormat {
    if matches!(supports_semver, ResolutionMode::Semver) {
        MetadataFormat::Abbreviated
    } else {
        MetadataFormat::Complete
    }
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

/// Queue a registry fetch for `(name, spec)` unless the store already has it.
pub(super) fn schedule_registry_fetch(
    state: &mut ManifestState,
    queues: &mut FetchQueues,
    name: String,
    spec: String,
    supports_semver: ResolutionMode,
    priority: FetchPriority,
) {
    let (real_name, real_spec) = normalize_spec(&name, &spec);
    if matches!(supports_semver, ResolutionMode::Semver) {
        if state.is_version_settled(&real_name, &real_spec) {
            return;
        }
        queues.push(
            ManifestJob::Version {
                name: real_name.clone(),
                spec: real_spec.clone(),
                fetch_spec: real_spec,
                format: version_metadata_format(supports_semver),
            },
            priority,
        );
    } else {
        if state.full.is_settled(&real_name) || state.versions_cache.contains_key(&real_name) {
            return;
        }
        queues.push(
            ManifestJob::Full {
                name: real_name,
                spec: Some(real_spec),
            },
            priority,
        );
    }
}

pub(super) fn enqueue_version_extract(
    queues: &mut FetchQueues,
    name: String,
    version: String,
    full: Arc<FullManifest>,
) {
    queues.push(
        ManifestJob::ExtractVersion {
            name,
            spec: version.clone(),
            version,
            full,
        },
        FetchPriority::Demand,
    );
}

pub(super) fn enqueue_version_fetch(
    queues: &mut FetchQueues,
    name: String,
    fetch_spec: String,
    supports_semver: ResolutionMode,
) {
    queues.push(
        ManifestJob::Version {
            name,
            spec: fetch_spec.clone(),
            fetch_spec,
            format: version_metadata_format(supports_semver),
        },
        FetchPriority::Demand,
    );
}

pub(super) fn schedule_transitive_prefetches(
    state: &mut ManifestState,
    queues: &mut FetchQueues,
    manifest: &CoreVersionManifest,
    peer_deps: PeerDeps,
    supports_semver: ResolutionMode,
) {
    for (name, spec) in collect_registry_prefetches(manifest, peer_deps) {
        schedule_registry_fetch(
            state,
            queues,
            name,
            spec,
            supports_semver,
            FetchPriority::Prefetch,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::manifest::{CoreVersionManifest, FullManifest};
    use crate::model::package_json::PackageJson;
    use crate::resolver::builder::resolve;
    use crate::service::{ManifestJob, ManifestJobDone};
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
}
