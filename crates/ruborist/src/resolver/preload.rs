//! Parallel manifest preloading via worker pool.
//!
//! Architecture: N long-lived `tokio::spawn` workers pulling work from a
//! shared `SegQueue`. Replaces the prior `FuturesUnordered` design that
//! had main task own the futures and poll them cooperatively, which
//! capped effective parallelism at ~55-60 even when standalone
//! manifest-bench (same reqwest stack, no resolver overhead) sustained
//! 90+ concurrent at the same cap. The deeper `await` chain inside
//! `resolve_package` (registry cache check + `OnceMap::get_or_init` +
//! `RetryIf` + `request.send()` + `bytes()` + parse-spawn_blocking)
//! made every yielded poll round-trip through the main task —
//! starving the dispatch refill. Worker tasks run on tokio's global
//! pool so each future progresses independently.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_queue::SegQueue;
use dashmap::DashSet;
use tokio::sync::{Notify, mpsc};

use crate::model::manifest::CoreVersionManifest;
use crate::model::node::PeerDeps;
use crate::resolver::registry::ResolveError;
use crate::resolver::registry::resolve_package;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::RegistryClient;

/// Default concurrency limit for manifest fetching.
///
/// Preload now runs N long-lived worker tasks; this is N. Each worker
/// processes one resolve_package at a time on tokio's global pool.
pub const DEFAULT_CONCURRENCY: usize = 256;

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

/// Result message sent from worker to main task.
type Completion<E> = (
    String,
    Result<crate::traits::registry::ResolvedPackage, ResolveError<E>>,
);

/// Preload all package manifests in parallel via a tokio worker pool.
///
/// `registry` is moved in and shared across workers via `Arc`. Each
/// worker is a long-lived spawned task that pulls work items from a
/// shared lock-free `SegQueue` until both the queue and the in-flight
/// counter reach zero, signalling end-of-phase.
///
/// `receiver` and `on_manifest` run on the main task only — they do
/// not need to be `Send`/`Sync`.
pub async fn preload_manifests<R, E, F>(
    initial_deps: Vec<Dep>,
    registry: Arc<R>,
    config: PreloadConfig,
    receiver: &E,
    mut on_manifest: F,
) -> PreloadStats
where
    R: RegistryClient + Send + Sync + 'static,
    R::Error: Send,
    E: EventReceiver,
    F: FnMut(&str, Arc<CoreVersionManifest>),
{
    let mut stats = PreloadStats::default();

    // Shared work queue and dedup set.
    let pending: Arc<SegQueue<Dep>> = Arc::new(SegQueue::new());
    let processed: Arc<DashSet<String>> = Arc::new(DashSet::new());

    // Counters for global termination — when `dispatched == completed`
    // and the queue is empty, the phase is done.
    let dispatched: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let completed: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let shutdown: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // Wake-up signal for workers parked on an empty queue.
    let notify: Arc<Notify> = Arc::new(Notify::new());

    // Result channel — workers send completions, main task drains.
    let (result_tx, mut result_rx) = mpsc::unbounded_channel::<Completion<R::Error>>();

    // Seed the queue with initial deps (deduped via DashSet).
    for dep in initial_deps {
        let key = format!("{}@{}", dep.0, dep.1);
        if processed.insert(key) {
            pending.push(dep);
            dispatched.fetch_add(1, Ordering::Relaxed);
        }
    }
    let initial_count = dispatched.load(Ordering::Relaxed);
    let concurrency = config.concurrency.max(1);

    tracing::debug!(
        "Preload: {} initial deps, concurrency={}, mode=worker-pool",
        initial_count,
        concurrency
    );

    // Spawn N long-lived workers. Each loops: pop -> resolve -> push transitives.
    for _ in 0..concurrency {
        let pending = Arc::clone(&pending);
        let processed = Arc::clone(&processed);
        let dispatched = Arc::clone(&dispatched);
        let completed = Arc::clone(&completed);
        let shutdown = Arc::clone(&shutdown);
        let notify = Arc::clone(&notify);
        let result_tx = result_tx.clone();
        let registry = Arc::clone(&registry);
        let config_for_worker = config.clone();

        tokio::spawn(async move {
            loop {
                // Try fetching work first — fast path when queue is hot.
                if let Some((name, spec)) = pending.pop() {
                    let result = resolve_package(&*registry, &name, &spec).await;

                    if let Ok(resolved) = &result {
                        let mut new_added = 0usize;
                        for tdep in extract_transitive_deps(&resolved.manifest, &config_for_worker)
                        {
                            let key = format!("{}@{}", tdep.0, tdep.1);
                            if processed.insert(key) {
                                pending.push(tdep);
                                new_added += 1;
                            }
                        }
                        if new_added > 0 {
                            dispatched.fetch_add(new_added, Ordering::Release);
                            // Wake parked workers so they pick up the new work
                            // before checking the termination condition.
                            notify.notify_waiters();
                        }
                    }

                    if result_tx.send((name, result)).is_err() {
                        // Main task dropped the receiver — done collecting.
                        break;
                    }
                    let done = completed.fetch_add(1, Ordering::AcqRel) + 1;

                    // After completion, check global done condition. The
                    // `Acquire` on dispatched pairs with the `Release` in
                    // the producer add above, so we won't miss late
                    // transitives queued by a sibling worker.
                    if done == dispatched.load(Ordering::Acquire) && pending.is_empty() {
                        shutdown.store(true, Ordering::Release);
                        notify.notify_waiters();
                        break;
                    }
                    continue;
                }

                // Queue empty — register interest before re-checking, then
                // park. The Notify+enable() pattern guarantees we won't
                // miss a `notify_waiters()` racing with our park.
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                let notified = notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if !pending.is_empty() {
                    continue;
                }
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                if completed.load(Ordering::Acquire) == dispatched.load(Ordering::Acquire) {
                    shutdown.store(true, Ordering::Release);
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

    receiver.on_event(BuildEvent::PreloadStart {
        count: initial_count,
    });

    // Main task: drain completions, run user callbacks.
    while let Some((name, result)) = result_rx.recv().await {
        match result {
            Ok(resolved) => {
                stats.success_count += 1;

                receiver.on_event(BuildEvent::PreloadProgress {
                    name: &name,
                    version: &resolved.version,
                    current: stats.success_count,
                });
                receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));

                on_manifest(&name, resolved.manifest);
            }
            Err(e) => {
                stats.failed_count += 1;
                tracing::debug!("Failed to preload {}: {}", name, e);
            }
        }
    }

    stats.total_processed = processed.len();

    receiver.on_event(BuildEvent::PreloadComplete {
        success: stats.success_count,
        failed: stats.failed_count,
    });

    stats
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::manifest::CoreVersionManifest;
    use crate::traits::progress::NoopReceiver;
    use crate::traits::registry::mock::MockRegistryClient;
    use std::collections::HashMap;
    use std::sync::Mutex;

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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_preload_single() {
        let mut registry = MockRegistryClient::new();
        registry.add_package("lodash", "4.17.21", manifest("lodash", "4.17.21"));

        let cache: Arc<Mutex<HashMap<String, Arc<CoreVersionManifest>>>> = Default::default();
        let cache_clone = Arc::clone(&cache);

        let stats = preload_manifests(
            vec![("lodash".into(), "^4.17.0".into())],
            Arc::new(registry),
            PreloadConfig::default(),
            &NoopReceiver,
            move |name, m| {
                cache_clone.lock().unwrap().insert(name.into(), m);
            },
        )
        .await;

        assert_eq!(stats.success_count, 1);
        assert!(cache.lock().unwrap().contains_key("lodash"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_preload_transitive() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "a",
            "1.0.0",
            manifest_with_deps("a", "1.0.0", vec![("b", "^1.0.0")]),
        );
        registry.add_package("b", "1.0.0", manifest("b", "1.0.0"));

        let cache: Arc<Mutex<HashMap<String, Arc<CoreVersionManifest>>>> = Default::default();
        let cache_clone = Arc::clone(&cache);

        let stats = preload_manifests(
            vec![("a".into(), "^1.0.0".into())],
            Arc::new(registry),
            PreloadConfig::default(),
            &NoopReceiver,
            move |name, m| {
                cache_clone.lock().unwrap().insert(name.into(), m);
            },
        )
        .await;

        assert_eq!(stats.success_count, 2);
        assert!(cache.lock().unwrap().contains_key("a"));
        assert!(cache.lock().unwrap().contains_key("b"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_preload_missing() {
        let registry = MockRegistryClient::new();
        let cache: Arc<Mutex<HashMap<String, Arc<CoreVersionManifest>>>> = Default::default();
        let cache_clone = Arc::clone(&cache);

        let stats = preload_manifests(
            vec![("nonexistent".into(), "^1.0.0".into())],
            Arc::new(registry),
            PreloadConfig::default(),
            &NoopReceiver,
            move |name, m| {
                cache_clone.lock().unwrap().insert(name.into(), m);
            },
        )
        .await;

        assert_eq!(stats.failed_count, 1);
        assert!(cache.lock().unwrap().is_empty());
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
