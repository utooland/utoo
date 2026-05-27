//! The fetch pipeline: turn queued jobs into provider futures, keep the
//! in-flight set saturated up to the concurrency limit, and apply completed
//! fetches back to the store (cache, wake waiters, schedule prefetches).

use std::collections::VecDeque;
use std::sync::Arc;

use futures::stream::FuturesUnordered;
use petgraph::graph::NodeIndex;

use crate::model::node::PeerDeps;
use crate::resolver::edges::DependencyEdgeInfo;
use crate::resolver::registry::ResolveError;
use crate::service::{ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider};
use crate::traits::registry::RegistryError;

use super::super::queue::{FetchDone, FetchFuture, FetchKey, FetchQueues};
use super::super::state::ManifestState;
use super::plan::ResolutionMode;
use super::schedule::schedule_transitive_prefetches;

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

fn fetch_registry_manifest<R>(registry: R, request: ManifestJob) -> FetchFuture
where
    R: ManifestProvider,
{
    Box::pin(fetch_registry_manifest_inner(registry, request))
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
