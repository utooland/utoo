//! Parallel manifest preloading for dependency resolution.
//!
//! Uses FuturesUnordered for true streaming concurrency: when a package resolves,
//! its transitive dependencies are immediately added to the queue.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::manifest::CoreVersionManifest;
use crate::model::node::PeerDeps;
use crate::resolver::registry::resolve_package;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::RegistryClient;

/// Default concurrency limit for manifest fetching.
///
/// Raised from 64 to 256 after pcap comparison against bun showed bun opens
/// ~256 parallel TCP connections during a cold install (typically 4 IPs × 64
/// conn each), while utoo's 64-cap kept us at roughly 1/4 the effective
/// parallelism even after the DNS round-robin fix. Overridable via
/// `--manifests-concurrency-limit`.
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

/// Preload all package manifests in parallel with streaming concurrency.
pub async fn preload_manifests<R, E, F>(
    initial_deps: Vec<Dep>,
    registry: &R,
    config: PreloadConfig,
    receiver: &E,
    mut on_manifest: F,
) -> PreloadStats
where
    R: RegistryClient,
    E: EventReceiver,
    F: FnMut(&str, Arc<CoreVersionManifest>),
{
    let mut stats = PreloadStats::default();
    // Per-iteration processing time on the main task (in μs). Each sample
    // covers the window from `futures.next().await` returning to the next
    // iteration's `futures.next().await` being re-entered — i.e. the
    // serial CPU cost for stats + event emission + callback + refill-loop
    // + next iteration setup. Printed as a histogram at shutdown.
    let mut proc_us: Vec<u32> = Vec::new();
    let mut processed: HashSet<String> = HashSet::new();
    // Shared pending queue: each in-flight future extracts its own
    // transitive deps on the blocking pool and pushes them here, so the
    // main loop never does CPU-bound dep-graph walking between `await`
    // points. pcap showed utoo's active-stream count oscillating
    // 11..64 — those dips correspond to bursts of post-processing CPU
    // on the single-task main loop; bun stays flat at 64.
    let pending = Arc::new(Mutex::new(VecDeque::<Dep>::from(initial_deps)));
    let concurrency = config.concurrency;

    tracing::debug!(
        "Preload: {} initial deps, concurrency={}",
        pending.lock().unwrap().len(),
        concurrency
    );

    let mut futures = FuturesUnordered::new();
    let mut in_flight = 0usize;
    let mut started = false;

    loop {
        // Fill up to concurrency limit
        while in_flight < concurrency {
            let item = loop {
                let mut queue = pending.lock().unwrap();
                let Some((name, spec)) = queue.pop_front() else {
                    break None;
                };
                drop(queue);
                let key = format!("{}@{}", name, spec);
                if !processed.contains(&key) {
                    processed.insert(key);
                    break Some((name, spec));
                }
            };

            let Some((name, spec)) = item else {
                break;
            };

            if !started {
                receiver.on_event(BuildEvent::PreloadStart { count: 1 });
                started = true;
            } else {
                receiver.on_event(BuildEvent::PreloadQueued { count: 1 });
            }

            receiver.on_event(BuildEvent::PreloadFetching { name: &name });

            let pending_for_task = Arc::clone(&pending);
            let config_for_task = config.clone();
            futures.push(async move {
                let start = tokio::time::Instant::now();
                let result = resolve_package(registry, &name, &spec).await;
                let elapsed_ms = start.elapsed().as_millis() as u64;

                // Off the hot async path: extract the transitive dep list
                // (iterates + clones strings for ~10 deps per manifest) on
                // the blocking pool, so the single main task can keep
                // dispatching new network requests without this CPU stall.
                if let Ok(resolved) = &result {
                    let manifest = Arc::clone(&resolved.manifest);
                    let _ = tokio::task::spawn_blocking(move || {
                        let deps = extract_transitive_deps(&manifest, &config_for_task);
                        if let Ok(mut queue) = pending_for_task.lock() {
                            queue.extend(deps);
                        }
                    })
                    .await;
                }

                (name, result, elapsed_ms)
            });
            in_flight += 1;
        }

        if in_flight == 0 {
            break;
        }

        let Some((name, result, elapsed_ms)) = futures.next().await else {
            break;
        };
        let iter_start = tokio::time::Instant::now();
        in_flight -= 1;

        if stats.success_count == 0 && stats.failed_count == 0 {
            stats.min_request_ms = elapsed_ms;
            stats.max_request_ms = elapsed_ms;
        } else {
            stats.min_request_ms = stats.min_request_ms.min(elapsed_ms);
            stats.max_request_ms = stats.max_request_ms.max(elapsed_ms);
        }
        stats.total_request_ms += elapsed_ms;

        match result {
            Ok(resolved) => {
                stats.success_count += 1;
                tracing::debug!("Preloaded {}@{}", name, resolved.version);

                receiver.on_event(BuildEvent::PreloadProgress {
                    name: &name,
                    version: &resolved.version,
                    current: stats.success_count,
                });

                // Send PackageResolved event for pipeline downloading
                receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));

                on_manifest(&name, resolved.manifest);
            }
            Err(e) => {
                stats.failed_count += 1;
                tracing::debug!("Failed to preload {}: {}", name, e);
            }
        }
        proc_us.push(iter_start.elapsed().as_micros().min(u32::MAX as u128) as u32);
    }

    stats.total_processed = processed.len();

    // Dump the per-iteration processing-time histogram. This is the
    // serial main-task cost that competes with the async poll of the
    // in-flight request pool; dips in active-stream count correlate
    // with spikes in these samples.
    if !proc_us.is_empty() {
        let mut v = proc_us.clone();
        v.sort_unstable();
        let pct = |p: f64| -> u32 {
            let idx = ((p * v.len() as f64) as usize).min(v.len() - 1);
            v[idx]
        };
        let sum: u64 = v.iter().map(|&x| x as u64).sum();
        tracing::info!(
            "preload proc_us (n={}): p50={} p90={} p99={} max={} sum={}us avg={}us",
            v.len(),
            pct(0.50),
            pct(0.90),
            pct(0.99),
            pct(1.0),
            sum,
            sum / v.len() as u64
        );
    }

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
            &registry,
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
            &registry,
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
            &registry,
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
