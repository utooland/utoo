//! Parallel manifest preloading for dependency resolution.
//!
//! Uses FuturesUnordered for true streaming concurrency: when a package resolves,
//! its transitive dependencies are immediately added to the queue.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use crossbeam_queue::SegQueue;
use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::manifest::CoreVersionManifest;
use crate::model::node::PeerDeps;
use crate::resolver::registry::resolve_package;
use crate::service::http::{
    finish_http_trace, finish_parse_trace, start_http_trace, start_parse_trace,
};
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
    let mut processed: HashSet<String> = HashSet::new();
    let preload_wall_start = Instant::now();
    start_http_trace();
    start_parse_trace();
    // Shared pending queue: each in-flight future pushes its transitive
    // deps here when it completes, and the main task pops from here to
    // refill the concurrency window.
    //
    // Was `Arc<Mutex<VecDeque<Dep>>>`. Timer histogram on the preload
    // pipeline (aca2c337) showed `first_poll_gap_us` avg 18.7 ms per
    // future — the time between `futures.push()` and the future's
    // actual first poll. With 128 completing futures all holding the
    // pending mutex to append transitives while the main task is trying
    // to acquire the same lock to pop and refill, the fill phase
    // serialised at ~100 μs per iteration × 128 refills ≈ 13-19 ms of
    // lock contention per batch. That's exactly the observed gap.
    //
    // `crossbeam_queue::SegQueue` is a lock-free MPMC queue. Push and
    // pop are wait-free; producers and consumers never block each
    // other. Eliminates the entire contention pocket.
    let pending: Arc<SegQueue<Dep>> = Arc::new(SegQueue::new());
    let initial_count = initial_deps.len();
    for dep in initial_deps {
        pending.push(dep);
    }
    let concurrency = config.concurrency;

    tracing::debug!(
        "Preload: {} initial deps, concurrency={}",
        initial_count,
        concurrency
    );

    let mut futures = FuturesUnordered::new();
    let mut in_flight = 0usize;
    let mut started = false;

    loop {
        // Fill up to concurrency limit
        while in_flight < concurrency {
            let item = loop {
                let Some((name, spec)) = pending.pop() else {
                    break None;
                };
                // Dedup by name only. The registry's per-name OnceMap
                // already coalesces concurrent fetches of the same
                // package, and `resolve_full_manifest` returns the full
                // version list — so a second `(lodash, ^1.0)` and
                // `(lodash, ^2.0)` would hit the same cache entry. Spec
                // is irrelevant at this layer.
                //
                // Name-only avoids the `format!("{}@{}", ...)` string
                // alloc per pop. Standalone manifest-bench reaches
                // avg_conc=92 at cap=128; ruborist sits at 70. The
                // dedup format! was a measurable fraction of that gap
                // — ~10 000 allocs on the main task hot path.
                if !processed.contains(name.as_str()) {
                    processed.insert(name.clone());
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

                // Push transitives directly into the lock-free SegQueue.
                // Each `push` is a wait-free O(1) operation; no
                // contention with either other completing futures or
                // the main task's fill-phase pops.
                if let Ok(resolved) = &result {
                    for dep in extract_transitive_deps(&resolved.manifest, &config_for_task) {
                        pending_for_task.push(dep);
                    }
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
    }

    stats.total_processed = processed.len();

    receiver.on_event(BuildEvent::PreloadComplete {
        success: stats.success_count,
        failed: stats.failed_count,
    });

    let preload_wall_ms = preload_wall_start.elapsed().as_millis();
    let intervals = finish_http_trace();
    log_http_diagnostics(&intervals, preload_wall_ms);
    let parses = finish_parse_trace();
    log_parse_diagnostics(&parses);

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

/// Summarise captured HTTP intervals from the preload phase and log one
/// info-level line. Splits total preload wall into:
///
/// - `wall` — first HTTP send → last HTTP body end (pure network window)
/// - `busy` — interval union (time at least one request was in-flight)
/// - `sum`  — Σ per-request `(end − start)` (total req-level time)
/// - `cpu_tail` = preload_total − wall (our CPU work after last body)
/// - `avg_conc` = sum / busy (effective parallelism over busy window)
/// - percentiles of per-request latency
///
/// If `wall ≫ bun_equivalent` the bottleneck is network. If `cpu_tail` is
/// large the resolver is blocking after HTTP completes. If `avg_conc` is
/// well below the configured limit the pipeline isn't actually filling.
fn log_http_diagnostics(intervals: &[(Instant, Instant)], preload_wall_ms: u128) {
    if intervals.is_empty() {
        tracing::info!(
            "Preload HTTP diag: no requests captured (total wall {}ms)",
            preload_wall_ms
        );
        return;
    }

    let mut spans: Vec<(Instant, Instant)> = intervals.to_vec();
    spans.sort_by_key(|(s, _)| *s);

    let first_start = spans.first().unwrap().0;
    let last_end = spans.iter().map(|(_, e)| *e).max().unwrap();
    let wall = last_end.duration_since(first_start).as_millis();

    let sum: u128 = spans
        .iter()
        .map(|(s, e)| e.duration_since(*s).as_micros())
        .sum();

    // Interval union: sweep sorted spans, merging overlaps.
    let mut busy_us: u128 = 0;
    let (mut cur_s, mut cur_e) = spans[0];
    for &(s, e) in &spans[1..] {
        if s <= cur_e {
            if e > cur_e {
                cur_e = e;
            }
        } else {
            busy_us += cur_e.duration_since(cur_s).as_micros();
            cur_s = s;
            cur_e = e;
        }
    }
    busy_us += cur_e.duration_since(cur_s).as_micros();

    let mut per_req_us: Vec<u128> = spans
        .iter()
        .map(|(s, e)| e.duration_since(*s).as_micros())
        .collect();
    per_req_us.sort_unstable();
    let n = per_req_us.len();
    let p50 = per_req_us[n / 2];
    let p95 = per_req_us[(n * 95).div_ceil(100).saturating_sub(1)];
    let max = *per_req_us.last().unwrap();

    let cpu_tail_ms = preload_wall_ms.saturating_sub(wall);
    let avg_conc = if busy_us > 0 {
        sum as f64 / busy_us as f64
    } else {
        0.0
    };

    tracing::info!(
        "Preload HTTP diag: n={} wall={}ms busy={}ms ({:.0}% of wall) sum={}ms avg_conc={:.1} p50={}ms p95={}ms max={}ms cpu_tail={}ms",
        n,
        wall,
        busy_us / 1000,
        if wall > 0 {
            100.0 * (busy_us as f64 / 1000.0) / wall as f64
        } else {
            0.0
        },
        sum / 1000,
        avg_conc,
        p50 / 1000,
        p95 / 1000,
        max / 1000,
        cpu_tail_ms,
    );
}

/// Summarise parse timing. Splits each parse into:
///
/// - `queue_wait` — `spawn_blocking` dispatch → closure exec_start
///   (blocking-pool queue time)
/// - `exec`       — exec_start → exec_end (actual simd_json work)
///
/// If `queue_wait p50 ≫ exec p50`, the blocking pool is the bottleneck —
/// parses are stacking up behind 4-thread capacity (on CI) and dragging
/// `resolve_package` awaits, which caps the outer concurrency.
fn log_parse_diagnostics(parses: &[(Instant, Instant, Instant)]) {
    if parses.is_empty() {
        return;
    }

    let mut queue_us: Vec<u128> = parses
        .iter()
        .map(|(q, s, _)| s.duration_since(*q).as_micros())
        .collect();
    let mut exec_us: Vec<u128> = parses
        .iter()
        .map(|(_, s, e)| e.duration_since(*s).as_micros())
        .collect();
    queue_us.sort_unstable();
    exec_us.sort_unstable();

    let n = parses.len();
    let pct = |v: &[u128], p: usize| v[(n * p).div_ceil(100).saturating_sub(1)];
    let sum_queue: u128 = queue_us.iter().sum();
    let sum_exec: u128 = exec_us.iter().sum();

    // Exec wall via interval union over (exec_start, exec_end) — time
    // when ≥1 blocking worker was actively parsing.
    let mut spans: Vec<(Instant, Instant)> = parses.iter().map(|(_, s, e)| (*s, *e)).collect();
    spans.sort_by_key(|(s, _)| *s);
    let (mut cur_s, mut cur_e) = spans[0];
    let mut exec_busy_us: u128 = 0;
    for &(s, e) in &spans[1..] {
        if s <= cur_e {
            if e > cur_e {
                cur_e = e;
            }
        } else {
            exec_busy_us += cur_e.duration_since(cur_s).as_micros();
            cur_s = s;
            cur_e = e;
        }
    }
    exec_busy_us += cur_e.duration_since(cur_s).as_micros();
    let avg_exec_parallelism = if exec_busy_us > 0 {
        sum_exec as f64 / exec_busy_us as f64
    } else {
        0.0
    };

    tracing::info!(
        "Preload parse diag: n={} queue(p50={}ms p95={}ms max={}ms sum={}ms) exec(p50={}ms p95={}ms max={}ms sum={}ms) exec_busy={}ms avg_parallel={:.1}",
        n,
        queue_us[n / 2] / 1000,
        pct(&queue_us, 95) / 1000,
        queue_us.last().unwrap() / 1000,
        sum_queue / 1000,
        exec_us[n / 2] / 1000,
        pct(&exec_us, 95) / 1000,
        exec_us.last().unwrap() / 1000,
        sum_exec / 1000,
        exec_busy_us / 1000,
        avg_exec_parallelism,
    );
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
