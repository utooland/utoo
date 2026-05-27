//! The BFS driver loop. Walks the dependency graph level by level, drives the
//! fetch pipeline, and feeds resolved manifests back into the graph. Owns the
//! per-run [`ManifestState`] store and [`FetchQueues`] scheduler.

use std::collections::VecDeque;
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use petgraph::graph::NodeIndex;

use crate::model::graph::{DependencyGraph, FindResult};
use crate::model::manifest::CoreVersionManifest;
use crate::model::manifest::NodeManifest;
use crate::model::node::{DevDeps, EdgeType};
use crate::resolver::builder::{
    BuildDepsConfig, ProcessResult, create_package_node, process_dependency,
    update_node_type_from_edge,
};
use crate::resolver::edges::{
    DependencyEdgeInfo, EdgeContext, add_edges_from, collect_unresolved_edges,
};
use crate::resolver::registry::ResolveError;
use crate::resolver::semver::normalize_spec;
use crate::service::ManifestProvider;
use crate::spec::SpecStr;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::{RegistryClient, ResolvedPackage};

use super::super::queue::{FetchFuture, FetchPriority, FetchQueues};
use super::super::state::{ManifestState, ResolverManifestCache};
use super::pipeline::{apply_fetch_result, pump_fetches, registry_error};
use super::plan::{EdgeStep, FetchPlan, ResolutionMode, WaitKey, plan_edge};
use super::schedule::{enqueue_version_extract, enqueue_version_fetch, schedule_registry_fetch};

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
                    plan_edge::<R::Error>(&state, &edge, &real_name, &real_spec, supports_semver)
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

    #[async_trait::async_trait(?Send)]
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
