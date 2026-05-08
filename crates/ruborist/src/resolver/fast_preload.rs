//! Lean parallel manifest fetcher modeled on `manifest-bench`.
//!
//! Bypasses [`crate::service::registry::UnifiedRegistry`] — and therefore
//! its `OnceMap` gates, [`crate::service::store::ManifestStore`] writes,
//! and `EventReceiver` event dispatch — to drive a flat
//! `FuturesUnordered` over [`crate::service::manifest::fetch_full_manifest`]
//! plus a rayon-dispatched per-spec settle. The warm
//! [`crate::service::cache::MemoryCache`] it leaves behind makes the
//! subsequent BFS phase a pure cache-hit walk: no network, no rayon
//! re-parse hop on `extract_core_version`.
//!
//! Intended for the lockfile-only path (`utoo deps`) which has no
//! pipeline consumer for `BuildEvent::PackageResolved` — install paths
//! still go through [`super::preload::preload_manifests`] so the
//! pipeline keeps its early-start signal.
//!
//! ## Why settle is dispatched off-runtime
//!
//! A "settle" turns a freshly-fetched `FullManifest` plus a spec into a
//! `CoreVersionManifest` for one version, via `simd_json::to_borrowed_value`
//! over the manifest's raw bytes. That parse is 5–10ms per spec on a
//! 100KB body. Calling it inline on the tokio runtime (the v1 of this
//! module) starves the runtime worker — sibling fetches in flight stop
//! draining their sockets while the worker is parsing, which CI showed
//! as `avg_request` rising +3ms and `avg_parse` jumping 5→11ms vs the
//! UnifiedRegistry baseline. Routing settle through `rayon::spawn`
//! (the same path the `extract_core_version_off_runtime` helper takes)
//! keeps the runtime free to drive I/O.

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

/// Output of one in-flight future. The main loop merges fetch and settle
/// completions through a single `FuturesUnordered` so backpressure on
/// either side throttles the other naturally.
///
/// `Fetched` is boxed because `FetchManifestResult::Ok` carries a fully-
/// parsed `FullManifest` (`raw` bytes + parsed envelope), which makes
/// the variant large enough that clippy flags the size delta with
/// `Settled`. The cost is one heap allocation per fetched manifest;
/// trivial against the network round-trip we already paid.
#[allow(clippy::large_enum_variant)]
enum FastEvent {
    Fetched {
        name: String,
        primary_spec: String,
        result: anyhow::Result<FetchManifestResult>,
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

/// Resolve `(name, spec)` against `full` off the tokio runtime.
///
/// Returns the freshly-extracted version manifest's transitive deps so
/// the caller can extend its pending queue. The heavy
/// `simd_json::to_borrowed_value` parse runs inside
/// `extract_core_version_off_runtime`, which dispatches to rayon — same
/// path the BFS phase uses for cold extracts.
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
            return FastEvent::Settled {
                new_deps: extract_transitive_deps(&cached, peer_deps),
            };
        }
        let (resolved_version, core) =
            extract_core_version_off_runtime(Arc::clone(&full), resolved_version).await;
        let new_deps = match core {
            Some(core_arc) => {
                cache.set_version_manifest(name, resolved_version, Arc::clone(&core_arc));
                extract_transitive_deps(&core_arc, peer_deps)
            }
            None => Vec::new(),
        };
        FastEvent::Settled { new_deps }
    })
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
    // Specs we've already enqueued (or settled). Prevents duplicate
    // settles from re-walking the same transitive subtree.
    let mut seen_specs: HashSet<(String, String)> = HashSet::new();
    // Names whose full manifest is in flight or already cached.
    let mut fetched_names: HashSet<String> = HashSet::new();
    // Sibling specs that arrived while their package's full manifest
    // was still in flight. The fetch's completion handler drains this
    // bucket — we stash by name so the lookup is one HashMap probe.
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

            // Hot path: the full manifest is already cached (a sibling
            // spec for this name has already returned). Dispatch a
            // settle so the parse work runs on rayon, not on the tokio
            // worker — keeps the runtime free for ongoing fetches.
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
            // spec; the fetch's completion handler will dispatch its
            // settle.
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
                FastEvent::Fetched {
                    name,
                    primary_spec,
                    result,
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
                result,
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

                match result {
                    Ok(FetchManifestResult::Ok(manifest, _etag)) => {
                        stats.success_count += 1;
                        stats.fetched_names += 1;
                        let full_arc = Arc::new(manifest);
                        cache.set_full_manifest(name.clone(), Arc::clone(&full_arc));

                        // Primary settle.
                        futs.push(Box::pin(settle_future(
                            name.clone(),
                            primary_spec,
                            Arc::clone(&full_arc),
                            cache.clone(),
                            peer_deps,
                        )));

                        // Sibling settles that were stashed while the
                        // fetch was in flight.
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
                    Ok(FetchManifestResult::NotModified) => {
                        // No ETag was sent on these requests, so 304 is
                        // unreachable in practice; treat as soft failure.
                        stats.failed_count += 1;
                    }
                    Err(e) => {
                        stats.failed_count += 1;
                        tracing::debug!("fast_preload failed for {}: {}", name, e);
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
