//! Parallel manifest preloading via multi-thread worker pool.
//!
//! N long-lived `tokio::spawn` workers pull work items from a shared
//! lock-free `SegQueue`. Each spawned task is independent on tokio's
//! multi-thread runtime, so when one worker is parsing manifest JSON
//! (CPU-bound, simd_json), other workers can still drive their network
//! IO. Replaces the prior `FuturesUnordered` design where the main task
//! owned all preload futures and polled them cooperatively — every
//! per-future await continuation (cache check / OnceMap / RetryIf /
//! reqwest send / bytes / parse / transitive dispatch) ran on the same
//! single task, plus all parses serialised in that task's polling.
//!
//! Wasm32 fallback uses `wasm_bindgen_futures::spawn_local` since
//! `JsFuture` is `!Send`. Workers still run independently on the JS
//! event loop; the queue + Notify + mpsc termination story is identical.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_queue::SegQueue;
use dashmap::DashSet;
use tokio::sync::{Notify, mpsc};

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::model::manifest::CoreVersionManifest;
use crate::model::node::PeerDeps;
use crate::resolver::registry::ResolveError;
use crate::resolver::registry::resolve_package;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::{RegistryClient, ResolvedPackage};

/// Default concurrency limit for manifest fetching.
///
/// Number of long-lived `tokio::spawn` workers. Each processes one
/// `resolve_package` at a time on tokio's multi-thread runtime.
pub const DEFAULT_CONCURRENCY: usize = 64;

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

/// Preload all package manifests in parallel via a multi-thread worker pool.
///
/// `registry` is moved in and shared across workers via `Arc`. Each
/// worker is a long-lived spawned task that pulls work items from a
/// shared lock-free `SegQueue` until both the queue and the in-flight
/// counter reach zero, signalling end-of-phase. Each spawned task is
/// independent on tokio's multi-thread runtime — IO and CPU work
/// (simd_json parse) parallelise across cores.
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
    R: RegistryClient + MaybeSend + MaybeSync + 'static,
    R::Error: MaybeSend,
    E: EventReceiver,
    F: FnMut(&str, Arc<CoreVersionManifest>),
{
    let mut stats = PreloadStats::default();

    // Shared work queue and dedup set (lock-free, multi-thread safe).
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
        "Preload: {} initial deps, concurrency={}, mode=mt-worker-pool",
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

    for _ in 0..concurrency {
        let pending = Arc::clone(&pending);
        let processed = Arc::clone(&processed);
        let dispatched = Arc::clone(&dispatched);
        let completed = Arc::clone(&completed);
        let shutdown = Arc::clone(&shutdown);
        let notify = Arc::clone(&notify);
        let result_tx = result_tx.clone();
        let config_for_worker = config.clone();
        let registry = Arc::clone(&registry);

        let worker = async move {
            loop {
                // Try fetching work first — fast path when queue is hot.
                if let Some((name, spec)) = pending.pop() {
                    let start = tokio::time::Instant::now();
                    let result = resolve_package(&*registry, &name, &spec).await;
                    let elapsed_ms = start.elapsed().as_millis() as u64;

                    let new_added = if let Ok(resolved) = &result {
                        let mut count = 0usize;
                        for tdep in extract_transitive_deps(&resolved.manifest, &config_for_worker)
                        {
                            let key = format!("{}@{}", tdep.0, tdep.1);
                            if processed.insert(key) {
                                pending.push(tdep);
                                count += 1;
                            }
                        }
                        if count > 0 {
                            dispatched.fetch_add(count, Ordering::Release);
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
        };
        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(worker);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(worker);
    }
    // Drop the original sender so when all worker clones drop on exit, the
    // result channel closes and the main loop terminates.
    drop(result_tx);

    receiver.on_event(BuildEvent::PreloadStart {
        count: initial_count,
    });

    // Main task: drain completions, run user callbacks. Receiver/callback
    // run on this task only — they don't need to be Send/Sync.
    while let Some((name, result, elapsed_ms, new_added)) = result_rx.recv().await {
        if stats.success_count == 0 && stats.failed_count == 0 {
            stats.min_request_ms = elapsed_ms;
            stats.max_request_ms = elapsed_ms;
        } else {
            stats.min_request_ms = stats.min_request_ms.min(elapsed_ms);
            stats.max_request_ms = stats.max_request_ms.max(elapsed_ms);
        }
        stats.total_request_ms += elapsed_ms;

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
                receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));

                on_manifest(&name, resolved.manifest);
            }
            Err(e) => {
                stats.failed_count += 1;
                tracing::debug!("Failed to preload {}: {}", name, e);
            }
        }
    }

    // Snapshot the dedup set's size for the historical observable.
    stats.total_processed = {
        let mut set: HashSet<String> = HashSet::with_capacity(processed.len());
        for entry in processed.iter() {
            set.insert(entry.key().clone());
        }
        set.len()
    };

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
