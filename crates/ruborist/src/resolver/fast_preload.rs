//! Lean parallel manifest fetcher modeled on `manifest-bench`.
//!
//! Bypasses [`crate::service::registry::UnifiedRegistry`] — and therefore
//! its `OnceMap` gates, [`crate::service::store::ManifestStore`] writes,
//! and `EventReceiver` event dispatch — to drive a flat
//! `FuturesUnordered` over [`crate::service::manifest::fetch_full_manifest`]
//! plus a fused-into-fetch primary settle. The warm
//! [`crate::service::cache::MemoryCache`] it leaves behind makes the
//! subsequent BFS phase a pure cache-hit walk: no network, no rayon
//! re-parse hop on `extract_core_version`.
//!
//! Intended for the lockfile-only path (`utoo deps`) which has no
//! pipeline consumer for `BuildEvent::PackageResolved` — install paths
//! still go through [`super::preload::preload_manifests`] so the
//! pipeline keeps its early-start signal.
//!
//! ## Why settle is fused into the fetch task
//!
//! A "settle" turns a freshly-fetched `FullManifest` plus a spec into a
//! `CoreVersionManifest` for one version, via `simd_json::to_borrowed_value`
//! over the manifest's raw bytes. That parse is 5–10ms per spec on a
//! 100KB body.
//!
//! v1 ran settle inline on the tokio runtime worker — that starved
//! sibling fetches' I/O drive (CI showed `avg_request` +3ms,
//! `avg_parse` 5→11ms). v2 dispatched settle to rayon via a separate
//! `FuturesUnordered` future, which fixed the runtime starvation but
//! introduced a dispatch RTT: fetch lands → rayon settle queued → settle
//! pops → `pending` finally gets transitive deps. That round-trip held
//! the wave-shaped transitive walk back, capping `eff_parallel` at ~44
//! against a 96 cap.
//!
//! v3 (this) folds the primary settle into the fetch task itself via
//! `tokio::task::spawn_blocking`. The fetch task awaits both the
//! network round-trip and the version-extract on the same blocking
//! pool slot, then returns with the resolved `CoreVersionManifest`
//! attached. The main loop pulls a single `Fetched` event and
//! immediately extends `pending` — no separate settle pop. Sibling
//! specs (rare; same package, different range) still go through a
//! `Settled` future to keep the primary path lean.

use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::manifest::{CoreVersionManifest, FullManifest, extract_core_version_off_runtime};
use crate::model::node::PeerDeps;
use crate::resolver::preload::{Dep, PreloadConfig};
use crate::resolver::version::resolve_target_version;
use crate::service::{
    FetchManifestOptions, FetchManifestResult, MemoryCache, MetadataFormat, fetch_full_manifest,
};
use crate::spec::SpecStr;
use crate::util::FETCH_TIMINGS;

/// Statistics from the lean fetch loop. Mirrors `PreloadStats` shape so
/// the bench-grep regex stays the same.
#[derive(Debug, Default)]
pub struct FastPreloadStats {
    pub success_count: usize,
    pub failed_count: usize,
    pub fetched_names: usize,
    pub min_request_ms: u64,
    pub max_request_ms: u64,
    pub total_request_ms: u64,
}

/// One fetch's primary settle outcome — the resolved version + parsed
/// `CoreVersionManifest` for the spec the fetch was originally issued
/// for. `None` means the spec didn't match any version (caller treats
/// as soft skip).
type PrimarySettle = Option<(String, Arc<CoreVersionManifest>)>;

/// Outcome of a fetch task. Owning `Arc<FullManifest>` (rather than
/// `FetchManifestResult` by-value) means the fetch task can `Arc::clone`
/// once for the primary settle, then pass ownership along — no full
/// `FullManifest` clone (which would copy the 200-entry `time`
/// HashMap + the `versions` `Vec<String>` per fetch).
enum FetchOutcome {
    Ok(Arc<FullManifest>),
    NotModified,
    Err,
}

/// Output of one in-flight future. The main loop merges fetch and
/// sibling-settle completions through a single `FuturesUnordered`.
enum FastEvent {
    Fetched {
        name: String,
        primary_spec: String,
        outcome: FetchOutcome,
        primary_settle: PrimarySettle,
        elapsed_ms: u64,
    },
    Settled {
        new_deps: Vec<Dep>,
    },
}

type FastFut = Pin<Box<dyn std::future::Future<Output = FastEvent> + Send>>;

/// Collect dependencies from any deps map, filtering out non-registry specs.
fn collect_deps(map: Option<&HashMap<String, String>>) -> Vec<Dep> {
    map.into_iter()
        .flatten()
        .filter(|(_, spec)| spec.is_registry_spec())
        .map(|(name, spec)| (name.clone(), spec.clone()))
        .collect()
}

/// Extract transitive dependencies from a resolved manifest.
/// devDependencies are omitted (only the root installs devDeps).
fn extract_transitive_deps(manifest: &CoreVersionManifest, peer_deps: PeerDeps) -> Vec<Dep> {
    let mut deps = Vec::new();
    deps.extend(collect_deps(manifest.dependencies.as_ref()));
    if peer_deps == PeerDeps::Include {
        deps.extend(collect_deps(manifest.peer_dependencies.as_ref()));
    }
    deps.extend(collect_deps(manifest.optional_dependencies.as_ref()));
    deps
}

/// Off-runtime settle for a `(name, spec)` whose `FullManifest` is
/// already cached. Used for sibling specs — multiple ranges on the
/// same package — that arrive after the primary fetch has landed.
fn settle_future(
    name: String,
    spec: String,
    full: Arc<FullManifest>,
    cache: MemoryCache,
    peer_deps: PeerDeps,
) -> BoxFuture<'static, FastEvent> {
    Box::pin(async move {
        let resolved_version = match resolve_target_version((&*full).into(), &spec) {
            Ok(v) => v,
            Err(_) => return FastEvent::Settled { new_deps: vec![] },
        };
        if let Some(cached) = cache.get_version_manifest(&name, &resolved_version) {
            cache.set_version_manifest(name.clone(), spec.clone(), Arc::clone(&cached));
            return FastEvent::Settled {
                new_deps: extract_transitive_deps(&cached, peer_deps),
            };
        }
        let (resolved_version, core) =
            extract_core_version_off_runtime(Arc::clone(&full), resolved_version).await;
        let new_deps = match core {
            Some(core_arc) => {
                cache.set_version_manifest(name.clone(), spec.clone(), Arc::clone(&core_arc));
                cache.set_version_manifest(name, resolved_version, Arc::clone(&core_arc));
                extract_transitive_deps(&core_arc, peer_deps)
            }
            None => Vec::new(),
        };
        FastEvent::Settled { new_deps }
    })
}

/// Resolve `(name, spec)` against `full` on tokio's blocking pool.
///
/// Same shape as `extract_core_version_off_runtime` (which uses rayon),
/// but stays inside the fetch task so the result lands together with
/// the network round-trip — no separate `FuturesUnordered` pop, so
/// `pending` gets the transitive deps the moment the fetch event is
/// drained. Tokio's blocking pool has a 512-thread cap; rayon's is
/// `max(num_cpus, 8)`. With many primary settles arriving in waves,
/// the wider blocking pool absorbs the burst better than rayon would.
async fn resolve_primary_settle(spec: String, full: Arc<FullManifest>) -> PrimarySettle {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(move || {
            let resolved = resolve_target_version((&*full).into(), &spec).ok()?;
            let core = full.get_core_version(&resolved)?;
            Some((resolved, Arc::new(core)))
        })
        .await
        .ok()
        .flatten()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let resolved = resolve_target_version((&*full).into(), &spec).ok()?;
        let core = full.get_core_version(&resolved)?;
        Some((resolved, Arc::new(core)))
    }
}

/// Manifest-bench-style flat parallel fetch of all transitively-reachable
/// registry manifests. Populates `cache` with both `full_manifests` and
/// `version_manifests` slots so the subsequent BFS does no network and no
/// re-parse.
///
/// `initial_deps` should already be the union of root+workspace
/// registry edges, with non-registry specs filtered out.
pub async fn fast_preload(
    initial_deps: Vec<Dep>,
    registry_url: &str,
    cache: &MemoryCache,
    config: &PreloadConfig,
) -> FastPreloadStats {
    let mut stats = FastPreloadStats::default();
    let mut pending: VecDeque<Dep> = VecDeque::from(initial_deps);
    // Specs we've already enqueued. Prevents duplicate settles from
    // re-walking the same transitive subtree.
    let mut seen_specs: HashSet<(String, String)> = HashSet::new();
    // Names whose full manifest is in flight or already cached.
    let mut fetched_names: HashSet<String> = HashSet::new();
    // Sibling specs that arrived while their package's full manifest
    // was still in flight. The fetch's completion handler dispatches
    // settles for them, then drains this bucket.
    let mut deferred_by_name: HashMap<String, Vec<String>> = HashMap::new();
    let mut futs: FuturesUnordered<FastFut> = FuturesUnordered::new();
    let concurrency = config.concurrency;
    let peer_deps = config.peer_deps;

    loop {
        while futs.len() < concurrency {
            let Some((name, spec)) = pending.pop_front() else {
                break;
            };
            if !seen_specs.insert((name.clone(), spec.clone())) {
                continue;
            }

            // Hot path: a sibling spec for this name has already
            // returned, so the full manifest is cached. Settle on
            // rayon (off-runtime) — keeps the primary fetch path
            // (next branch) clean.
            if let Some(full) = cache.get_full_manifest(&name) {
                futs.push(Box::pin(settle_future(
                    name,
                    spec,
                    full,
                    cache.clone(),
                    peer_deps,
                )));
                continue;
            }

            // A fetch for this name is already in flight: stash this
            // sibling spec; the fetch's completion handler will
            // dispatch a settle for it.
            if !fetched_names.insert(name.clone()) {
                deferred_by_name.entry(name).or_default().push(spec);
                continue;
            }

            let registry_url = registry_url.to_string();
            let primary_spec = spec.clone();
            let n = name.clone();
            futs.push(Box::pin(async move {
                let start = tokio::time::Instant::now();
                let result = fetch_full_manifest(FetchManifestOptions {
                    registry_url: &registry_url,
                    name: &n,
                    format: MetadataFormat::Abbreviated,
                    etag: None,
                })
                .await;
                let elapsed_ms = start.elapsed().as_millis() as u64;
                // Fuse the primary settle into the same task so the
                // main loop sees the resolved version + transitive
                // deps in the same event — no extra `next().await` to
                // wait through the FuturesUnordered queue before
                // `pending` can refill.
                let (outcome, primary_settle) = match result {
                    Ok(FetchManifestResult::Ok(manifest, _etag)) => {
                        let full_arc = Arc::new(manifest);
                        let settle =
                            resolve_primary_settle(primary_spec.clone(), Arc::clone(&full_arc))
                                .await;
                        (FetchOutcome::Ok(full_arc), settle)
                    }
                    Ok(FetchManifestResult::NotModified) => (FetchOutcome::NotModified, None),
                    Err(e) => {
                        tracing::debug!("fast_preload failed for {}: {}", n, e);
                        (FetchOutcome::Err, None)
                    }
                };
                FastEvent::Fetched {
                    name,
                    primary_spec,
                    outcome,
                    primary_settle,
                    elapsed_ms,
                }
            }));
        }

        if futs.is_empty() {
            break;
        }

        let Some(event) = futs.next().await else {
            break;
        };

        match event {
            FastEvent::Fetched {
                name,
                primary_spec,
                outcome,
                primary_settle,
                elapsed_ms,
            } => {
                if stats.success_count == 0 && stats.failed_count == 0 {
                    stats.min_request_ms = elapsed_ms;
                    stats.max_request_ms = elapsed_ms;
                } else {
                    stats.min_request_ms = stats.min_request_ms.min(elapsed_ms);
                    stats.max_request_ms = stats.max_request_ms.max(elapsed_ms);
                }
                stats.total_request_ms += elapsed_ms;

                match outcome {
                    FetchOutcome::Ok(full_arc) => {
                        stats.success_count += 1;
                        stats.fetched_names += 1;
                        cache.set_full_manifest(name.clone(), Arc::clone(&full_arc));

                        // Apply the primary settle (already done inside
                        // the fetch task via spawn_blocking) — populate
                        // both `(name, primary_spec)` and
                        // `(name, resolved_version)` cache slots so BFS
                        // hits the early-return at registry.rs:347 on
                        // its first probe, then extend `pending` with
                        // the spec's transitive deps.
                        if let Some((resolved_version, core_arc)) = primary_settle {
                            cache.set_version_manifest(
                                name.clone(),
                                primary_spec,
                                Arc::clone(&core_arc),
                            );
                            cache.set_version_manifest(
                                name.clone(),
                                resolved_version,
                                Arc::clone(&core_arc),
                            );
                            pending.extend(extract_transitive_deps(&core_arc, peer_deps));
                        }

                        // Sibling specs that were stashed while the
                        // fetch was in flight: dispatch each as a
                        // separate settle future.
                        if let Some(siblings) = deferred_by_name.remove(&name) {
                            for sibling_spec in siblings {
                                futs.push(Box::pin(settle_future(
                                    name.clone(),
                                    sibling_spec,
                                    Arc::clone(&full_arc),
                                    cache.clone(),
                                    peer_deps,
                                )));
                            }
                        }
                    }
                    FetchOutcome::NotModified | FetchOutcome::Err => {
                        // 304 is unreachable in practice (no ETag sent);
                        // both branches treated as soft failure.
                        stats.failed_count += 1;
                    }
                }
            }
            FastEvent::Settled { new_deps } => {
                pending.extend(new_deps);
            }
        }
    }

    let total = stats.success_count + stats.failed_count;
    let avg_ms = if total > 0 {
        stats.total_request_ms / total as u64
    } else {
        0
    };
    tracing::info!(
        "p1-breakdown fast_preload n={} ok={} fail={} avg_req={}ms min={}ms max={}ms | {}",
        total,
        stats.success_count,
        stats.failed_count,
        avg_ms,
        stats.min_request_ms,
        stats.max_request_ms,
        FETCH_TIMINGS.snapshot().summary_line(),
    );

    stats
}
