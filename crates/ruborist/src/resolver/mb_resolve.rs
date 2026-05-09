//! Manifest-bench-style flat manifest fetcher (experimental new pipeline).
//!
//! A parallel-track alternative to [`super::fast_preload`], structured
//! to match `manifest-bench`'s main-loop shape as closely as
//! correctness allows. The hypothesis under test: `fast_preload`'s
//! eff_parallel caps at ~50 against a 96-cap because the main loop's
//! CPU work (FastEvent enum match + cache writes + sibling-deferred
//! bookkeeping + Box::pin allocation) competes with tokio runtime
//! workers for the 2 cores on GHA, stalling socket I/O drive.
//!
//! `mb_resolve` pushes ALL per-fetch work into the spawned future
//! itself (cache writes included) so the main loop is reduced to:
//!
//! ```ignore
//! while let Some(deps) = futs.next().await {
//!     pending.extend(deps);
//!     refill_to_cap(&mut futs, &mut pending, ...);
//! }
//! ```
//!
//! Sibling specs (multiple ranges on the same package) are NOT
//! deferred at queue level — if two specs for the same name race,
//! both fetch. This wastes a small number of network requests (~5-50
//! across a real install) but keeps the main loop's per-event cost
//! minimal (no HashMap probe / drain). The race converges: whichever
//! fetch lands first populates `full_manifests`; subsequent racers
//! find the cache hit on entry and short-circuit to a sibling-style
//! settle without re-fetching.
//!
//! Wiring: opt-in via `UTOO_RESOLVE=mb` env var. Both `utoo deps`
//! and `utoo install` route through this when set; install loses
//! pipelining (mb_fetch doesn't emit `PackageResolved` events) but
//! gains the lean main loop for resolve-phase A/B testing.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::PeerDeps;
use crate::resolver::preload::{Dep, PreloadConfig};
use crate::resolver::version::resolve_target_version;
use crate::service::{
    FetchManifestOptions, FetchWithSettleResult, MemoryCache, MetadataFormat,
    fetch_full_manifest_with_settle,
};
use crate::spec::SpecStr;
use crate::util::FETCH_TIMINGS;

#[derive(Debug, Default)]
pub struct MbFetchStats {
    pub success: usize,
    pub fail: usize,
}

/// Collect dependencies from a deps map, filtering non-registry specs.
fn collect_deps(map: Option<&std::collections::HashMap<String, String>>) -> Vec<Dep> {
    map.into_iter()
        .flatten()
        .filter(|(_, spec)| spec.is_registry_spec())
        .map(|(name, spec)| (name.clone(), spec.clone()))
        .collect()
}

fn extract_transitive(manifest: &CoreVersionManifest, peer_deps: PeerDeps) -> Vec<Dep> {
    let mut out = Vec::new();
    out.extend(collect_deps(manifest.dependencies.as_ref()));
    if peer_deps == PeerDeps::Include {
        out.extend(collect_deps(manifest.peer_dependencies.as_ref()));
    }
    out.extend(collect_deps(manifest.optional_dependencies.as_ref()));
    out
}

/// Settle one (name, spec) against an already-cached `FullManifest`.
/// Used for sibling specs (or racing-fetch losers) — extracts the
/// resolved version's `CoreVersionManifest` on the blocking pool,
/// populates both `(name, spec)` and `(name, resolved_version)` cache
/// slots so BFS hits the early-return fast path.
async fn settle_sibling(
    name: String,
    spec: String,
    full: Arc<FullManifest>,
    cache: MemoryCache,
    peer_deps: PeerDeps,
) -> Vec<Dep> {
    let Ok(resolved) = resolve_target_version((&*full).into(), &spec) else {
        return Vec::new();
    };
    if let Some(cached) = cache.get_version_manifest(&name, &resolved) {
        cache.set_version_manifest(name, spec, Arc::clone(&cached));
        return extract_transitive(&cached, peer_deps);
    }

    let resolved_for_parse = resolved.clone();
    let full_for_parse = Arc::clone(&full);
    let core_opt = tokio::task::spawn_blocking(move || {
        full_for_parse
            .get_core_version(&resolved_for_parse)
            .map(Arc::new)
    })
    .await
    .ok()
    .flatten();

    let Some(core_arc) = core_opt else {
        return Vec::new();
    };
    cache.set_version_manifest(name.clone(), spec, Arc::clone(&core_arc));
    cache.set_version_manifest(name, resolved, Arc::clone(&core_arc));
    extract_transitive(&core_arc, peer_deps)
}

/// Self-contained per-spec future. Either fetches `(name)`'s full
/// manifest from the registry (if not yet cached), or settles against
/// an already-cached one. In both cases it:
///   * writes `full_manifests` and `version_manifests` cache slots
///     for the resolved spec,
///   * returns the spec's transitive deps for the main loop to
///     enqueue.
///
/// Racing-fetch handling: two specs for the same name dispatched
/// concurrently both enter the fetch branch (no in-flight gate). The
/// second one re-issues a network round-trip; the cost is bounded by
/// the small number of sibling specs in real workloads (<2% in
/// ant-design-x). Last writer to `cache.set_full_manifest` wins;
/// content is identical so correctness is preserved.
async fn fetch_or_settle(
    name: String,
    spec: String,
    registry_url: String,
    cache: MemoryCache,
    peer_deps: PeerDeps,
) -> Vec<Dep> {
    // Sibling fast path: full manifest already cached.
    if let Some(full) = cache.get_full_manifest(&name) {
        return settle_sibling(name, spec, full, cache, peer_deps).await;
    }

    let result = fetch_full_manifest_with_settle(
        FetchManifestOptions {
            registry_url: &registry_url,
            name: &name,
            format: MetadataFormat::Abbreviated,
            etag: None,
        },
        &spec,
    )
    .await;

    let Ok(FetchWithSettleResult::Ok(payload)) = result else {
        return Vec::new();
    };

    let full_arc = Arc::new(payload.manifest);
    cache.set_full_manifest(name.clone(), Arc::clone(&full_arc));

    let Some((resolved, core_arc)) = payload.primary_settle else {
        return Vec::new();
    };
    cache.set_version_manifest(name.clone(), spec, Arc::clone(&core_arc));
    cache.set_version_manifest(name, resolved, Arc::clone(&core_arc));
    extract_transitive(&core_arc, peer_deps)
}

/// Manifest-bench-style flat parallel fetch. See module docs for the
/// rationale.
pub async fn mb_fetch(
    initial_deps: Vec<Dep>,
    registry_url: &str,
    cache: &MemoryCache,
    config: &PreloadConfig,
) -> MbFetchStats {
    let mut stats = MbFetchStats::default();
    let mut pending: VecDeque<Dep> = initial_deps.into();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut futs = FuturesUnordered::new();
    let cap = config.concurrency;
    let peer_deps = config.peer_deps;
    let registry_url = registry_url.to_string();

    let start = tokio::time::Instant::now();

    // Initial fill — same shape as the refill below.
    while futs.len() < cap {
        let Some((name, spec)) = pending.pop_front() else {
            break;
        };
        if !seen.insert((name.clone(), spec.clone())) {
            continue;
        }
        futs.push(Box::pin(fetch_or_settle(
            name,
            spec,
            registry_url.clone(),
            cache.clone(),
            peer_deps,
        )));
    }

    while let Some(transitive) = futs.next().await {
        if transitive.is_empty() {
            // Empty result is ambiguous (no transitive deps OR fetch
            // failed) — `MbFetchStats` only tracks success/fail at a
            // coarse level. The fetch-timings counters (recorded
            // inside `fetch_full_manifest_with_settle`) carry the
            // detailed per-fetch metrics.
            stats.fail += 1;
        } else {
            stats.success += 1;
        }
        pending.extend(transitive);

        // Refill — same body as the initial fill above.
        while futs.len() < cap {
            let Some((name, spec)) = pending.pop_front() else {
                break;
            };
            if !seen.insert((name.clone(), spec.clone())) {
                continue;
            }
            futs.push(Box::pin(fetch_or_settle(
                name,
                spec,
                registry_url.clone(),
                cache.clone(),
                peer_deps,
            )));
        }
    }

    let wall = start.elapsed();
    tracing::info!(
        "p1-breakdown mb_fetch wall={}ms ok={} fail={} | {}",
        wall.as_millis(),
        stats.success,
        stats.fail,
        FETCH_TIMINGS.snapshot().summary_line(),
    );

    stats
}
