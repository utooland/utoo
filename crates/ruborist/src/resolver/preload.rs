//! Parallel manifest preloading via single-thread worker pool.
//!
//! N long-lived `tokio::task::spawn_local` workers pull work items from a
//! shared `VecDeque`. Replaces the prior `FuturesUnordered` design where
//! the main task owned all preload futures and polled them cooperatively
//! — every per-future `await` continuation (cache check / OnceMap /
//! RetryIf / reqwest send / bytes / parse / transitive dispatch) ran on
//! the same single task, saturating it. CI ant-design preload sustained
//! avg_conc ~55-60 even when the standalone manifest-bench (same reqwest
//! stack, no resolver overhead) hit 90+ at the same nominal cap. Splitting
//! the futures into N independent `spawn_local` tasks lets tokio schedule
//! each future's continuation independently while still running all
//! workers on the current OS thread — no `Send`/`Sync` bound on the
//! registry trait, wasm-compatible.

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use tokio::sync::{Notify, mpsc};
use tokio::task::LocalSet;

use crate::model::manifest::CoreVersionManifest;
use crate::model::node::PeerDeps;
use crate::resolver::registry::ResolveError;
use crate::resolver::registry::resolve_package;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::{RegistryClient, ResolvedPackage};

/// Default concurrency limit for manifest fetching.
///
/// Number of long-lived `spawn_local` workers. Each processes one
/// `resolve_package` at a time on the current thread.
pub const DEFAULT_CONCURRENCY: usize = 128;

/// A dependency spec: (name, version_spec)
pub type Dep = (String, String);

/// Configuration for preload behavior
#[derive(Debug, Clone)]
pub struct PreloadConfig {
    /// How to handle peer dependencies.
    pub peer_deps: PeerDeps,
    /// Maximum number of concurrent manifest fetches
    pub concurrency: usize,
}

impl Default for PreloadConfig {
    fn default() -> Self {
        Self {
            peer_deps: PeerDeps::Skip,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }
}

/// Statistics from preload operation
#[derive(Debug, Default)]
pub struct PreloadStats {
    pub success_count: usize,
    pub failed_count: usize,
    pub total_processed: usize,
    pub min_request_ms: u64,
    pub max_request_ms: u64,
    pub total_request_ms: u64,
}

/// Collect dependencies from any deps map, filtering out non-registry specs.
fn collect_deps(map: Option<&std::collections::HashMap<String, String>>) -> Vec<Dep> {
    use crate::spec::SpecStr;
    map.into_iter()
        .flatten()
        .filter(|(_, spec)| spec.is_registry_spec())
        .map(|(name, spec)| (name.clone(), spec.clone()))
        .collect()
}

/// Extract transitive dependencies from a resolved manifest.
/// Note: devDependencies are NOT included (only root packages install devDeps).
fn extract_transitive_deps(manifest: &CoreVersionManifest, config: &PreloadConfig) -> Vec<Dep> {
    let mut deps = Vec::new();
    deps.extend(collect_deps(manifest.dependencies.as_ref()));
    if config.peer_deps == PeerDeps::Include {
        deps.extend(collect_deps(manifest.peer_dependencies.as_ref()));
    }
    deps.extend(collect_deps(manifest.optional_dependencies.as_ref()));
    deps
}

/// Result message sent from worker to main task: name, resolve result,
/// per-request elapsed ms, count of new transitives queued by this worker.
type Completion<E> = (String, Result<ResolvedPackage, ResolveError<E>>, u64, usize);

/// Preload all package manifests in parallel via a single-thread worker pool.
///
/// Spawns N long-lived `spawn_local` workers on a `LocalSet` that all share
/// a `Rc<RefCell<VecDeque<Dep>>>` work queue and a `Rc<RefCell<HashSet>>`
/// dedup set. Workers pull, call `resolve_package`, push transitives, and
/// send completions back via an unbounded mpsc channel which the main task
/// drains.
///
/// `registry` is moved in and shared across workers via `Rc` (single-thread
/// reference count, no `Send` required). `receiver` and `on_manifest` are
/// borrowed for the lifetime of the call.
pub async fn preload_manifests<R, E, F>(
    initial_deps: Vec<Dep>,
    registry: Rc<R>,
    config: PreloadConfig,
    receiver: &E,
    mut on_manifest: F,
) -> PreloadStats
where
    R: RegistryClient + 'static,
    E: EventReceiver,
    F: FnMut(&str, Arc<CoreVersionManifest>),
{
    let mut stats = PreloadStats::default();

    // Shared single-thread state. `Rc<RefCell<...>>` is fine here —
    // every worker is `spawn_local`-ed onto the same OS thread, so no
    // synchronisation primitives are needed.
    let pending: Rc<RefCell<VecDeque<Dep>>> = Rc::new(RefCell::new(VecDeque::new()));
    let processed: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));

    // Counters for global termination — when `dispatched == completed`
    // and the queue is empty, the phase is done.
    let dispatched: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let completed: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let shutdown: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Wake-up signal for workers parked on an empty queue.
    let notify: Rc<Notify> = Rc::new(Notify::new());

    // Result channel — workers send completions, main task drains.
    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<Completion<R::Error>>();

    // Seed the queue with initial deps (deduped via the shared set).
    {
        let mut p = pending.borrow_mut();
        let mut s = processed.borrow_mut();
        for dep in initial_deps {
            let key = format!("{}@{}", dep.0, dep.1);
            if s.insert(key) {
                p.push_back(dep);
                dispatched.set(dispatched.get() + 1);
            }
        }
    }
    let initial_count = dispatched.get();
    let concurrency = config.concurrency.max(1);

    tracing::debug!(
        "Preload: {} initial deps, concurrency={}, mode=spawn_local-pool",
        initial_count,
        concurrency
    );

    // Short-circuit if nothing was seeded.
    if initial_count == 0 {
        receiver.on_event(BuildEvent::PreloadStart { count: 0 });
        receiver.on_event(BuildEvent::PreloadComplete {
            success: 0,
            failed: 0,
        });
        return stats;
    }

    // Run all worker spawns + the main drain loop inside a LocalSet so
    // `spawn_local` is valid and worker futures can borrow `&R`.
    let local = LocalSet::new();

    for _ in 0..concurrency {
        let pending = Rc::clone(&pending);
        let processed = Rc::clone(&processed);
        let dispatched = Rc::clone(&dispatched);
        let completed = Rc::clone(&completed);
        let shutdown = Rc::clone(&shutdown);
        let notify = Rc::clone(&notify);
        let result_tx = result_tx.clone();
        let config_for_worker = config.clone();
        let registry = Rc::clone(&registry);

        local.spawn_local(async move {
            loop {
                // Try fetching work first — fast path when queue is hot.
                let work = pending.borrow_mut().pop_front();
                if let Some((name, spec)) = work {
                    let start = tokio::time::Instant::now();
                    let result = resolve_package(&*registry, &name, &spec).await;
                    let elapsed_ms = start.elapsed().as_millis() as u64;

                    let new_added = if let Ok(resolved) = &result {
                        let mut count = 0usize;
                        for tdep in extract_transitive_deps(&resolved.manifest, &config_for_worker)
                        {
                            let key = format!("{}@{}", tdep.0, tdep.1);
                            if processed.borrow_mut().insert(key) {
                                pending.borrow_mut().push_back(tdep);
                                count += 1;
                            }
                        }
                        if count > 0 {
                            dispatched.set(dispatched.get() + count);
                            // Wake parked workers so they pick up the new
                            // work before checking the termination condition.
                            notify.notify_waiters();
                        }
                        count
                    } else {
                        0
                    };

                    if result_tx
                        .send((name, result, elapsed_ms, new_added))
                        .is_err()
                    {
                        // Main task dropped the receiver — done.
                        break;
                    }
                    let done = completed.get() + 1;
                    completed.set(done);

                    // After completion, check global done condition.
                    if done == dispatched.get() && pending.borrow().is_empty() {
                        shutdown.set(true);
                        notify.notify_waiters();
                        break;
                    }
                    continue;
                }

                // Queue empty — register interest before re-checking, then
                // park. The Notify+enable() pattern guarantees we won't
                // miss a `notify_waiters()` racing with our park.
                if shutdown.get() {
                    break;
                }
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if !pending.borrow().is_empty() {
                    continue;
                }
                if shutdown.get() {
                    break;
                }
                if completed.get() == dispatched.get() {
                    shutdown.set(true);
                    notify.notify_waiters();
                    break;
                }
                notified.await;
            }
        });
    }
    // Drop the original sender so when all worker clones drop on exit, the
    // result channel closes and the main loop terminates.
    drop(result_tx);

    // Main drain loop — runs inside the LocalSet alongside the workers.
    local
        .run_until(async {
            receiver.on_event(BuildEvent::PreloadStart {
                count: initial_count,
            });

            while let Some((name, result, elapsed_ms, new_added)) = result_rx.recv().await {
                if stats.success_count == 0 && stats.failed_count == 0 {
                    stats.min_request_ms = elapsed_ms;
                    stats.max_request_ms = elapsed_ms;
                } else {
                    stats.min_request_ms = stats.min_request_ms.min(elapsed_ms);
                    stats.max_request_ms = stats.max_request_ms.max(elapsed_ms);
                }
                stats.total_request_ms += elapsed_ms;

                // Grow the progress bar length as transitives are discovered.
                if new_added > 0 {
                    receiver.on_event(BuildEvent::PreloadQueued { count: new_added });
                }

                match result {
                    Ok(resolved) => {
                        stats.success_count += 1;
                        tracing::debug!("Preloaded {}@{}", name, resolved.version);

                        receiver.on_event(BuildEvent::PreloadProgress {
                            name: &name,
                            version: &resolved.version,
                            current: stats.success_count,
                        });
                        receiver
                            .on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));

                        on_manifest(&name, resolved.manifest);
                    }
                    Err(e) => {
                        stats.failed_count += 1;
                        tracing::debug!("Failed to preload {}: {}", name, e);
                    }
                }
            }
        })
        .await;

    stats.total_processed = processed.borrow().len();

    receiver.on_event(BuildEvent::PreloadComplete {
        success: stats.success_count,
        failed: stats.failed_count,
    });

    let total = stats.success_count + stats.failed_count;
    let avg = if total > 0 {
        stats.total_request_ms / total as u64
    } else {
        0
    };
    tracing::debug!(
        "Preload stats: {} requests, min={}ms, max={}ms, avg={}ms, total={}ms",
        total,
        stats.min_request_ms,
        stats.max_request_ms,
        avg,
        stats.total_request_ms
    );

    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::manifest::CoreVersionManifest;
    use crate::traits::progress::NoopReceiver;
    use crate::traits::registry::mock::MockRegistryClient;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    fn manifest(name: &str, version: &str) -> CoreVersionManifest {
        CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        }
    }

    fn manifest_with_deps(
        name: &str,
        version: &str,
        deps: Vec<(&str, &str)>,
    ) -> CoreVersionManifest {
        CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            dependencies: Some(
                deps.into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            ),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_preload_single() {
        let mut registry = MockRegistryClient::new();
        registry.add_package("lodash", "4.17.21", manifest("lodash", "4.17.21"));

        let cache: Rc<RefCell<HashMap<String, Arc<CoreVersionManifest>>>> = Default::default();
        let cache_clone = Rc::clone(&cache);

        let stats = preload_manifests(
            vec![("lodash".into(), "^4.17.0".into())],
            Rc::new(registry),
            PreloadConfig::default(),
            &NoopReceiver,
            |name, m| {
                cache_clone.borrow_mut().insert(name.into(), m);
            },
        )
        .await;

        assert_eq!(stats.success_count, 1);
        assert!(cache.borrow().contains_key("lodash"));
    }

    #[tokio::test]
    async fn test_preload_transitive() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "a",
            "1.0.0",
            manifest_with_deps("a", "1.0.0", vec![("b", "^1.0.0")]),
        );
        registry.add_package("b", "1.0.0", manifest("b", "1.0.0"));

        let cache: Rc<RefCell<HashMap<String, Arc<CoreVersionManifest>>>> = Default::default();
        let cache_clone = Rc::clone(&cache);

        let stats = preload_manifests(
            vec![("a".into(), "^1.0.0".into())],
            Rc::new(registry),
            PreloadConfig::default(),
            &NoopReceiver,
            |name, m| {
                cache_clone.borrow_mut().insert(name.into(), m);
            },
        )
        .await;

        assert_eq!(stats.success_count, 2);
        assert!(cache.borrow().contains_key("a"));
        assert!(cache.borrow().contains_key("b"));
    }

    #[tokio::test]
    async fn test_preload_missing() {
        let registry = MockRegistryClient::new();
        let cache: Rc<RefCell<HashMap<String, Arc<CoreVersionManifest>>>> = Default::default();
        let cache_clone = Rc::clone(&cache);

        let stats = preload_manifests(
            vec![("nonexistent".into(), "^1.0.0".into())],
            Rc::new(registry),
            PreloadConfig::default(),
            &NoopReceiver,
            |name, m| {
                cache_clone.borrow_mut().insert(name.into(), m);
            },
        )
        .await;

        assert_eq!(stats.failed_count, 1);
        assert!(cache.borrow().is_empty());
    }

    #[test]
    fn test_is_registry_spec() {
        use crate::spec::SpecStr;

        // Local specs — not registry
        assert!(!"file:../foo".is_registry_spec());
        assert!(!"link:../foo".is_registry_spec());
        assert!(!"workspace:*".is_registry_spec());
        assert!(!"portal:../foo".is_registry_spec());

        // Git specs — not registry
        assert!(!"git+https://github.com/user/repo.git".is_registry_spec());
        assert!(!"git+ssh://git@github.com/user/repo.git".is_registry_spec());
        assert!(!"git+https://github.com/user/repo.git#main".is_registry_spec());
        assert!(!"git://github.com/user/repo.git".is_registry_spec());
        assert!(!"github:user/repo".is_registry_spec());
        assert!(!"github:user/repo#v1.0".is_registry_spec());

        // HTTP tarball specs — not registry
        assert!(!"https://example.com/pkg.tgz".is_registry_spec());
        assert!(!"http://example.com/pkg.tar.gz".is_registry_spec());
        assert!(!"https://example.com/pkg.tgz?v=1.0".is_registry_spec());

        // Bare GitHub shorthand — not registry
        assert!(!"user/repo".is_registry_spec());
        assert!(!"user/repo#v1.0".is_registry_spec());

        // Registry specs
        assert!("^1.0.0".is_registry_spec());
        assert!("latest".is_registry_spec());
        assert!("~2.0.0".is_registry_spec());
        assert!("@scope/pkg@1.0.0".is_registry_spec());
    }
}
