//! Lean parallel manifest fetcher modeled on `manifest-bench`.
//!
//! Bypasses [`crate::service::registry::UnifiedRegistry`] — and therefore
//! its `OnceMap` gates, [`crate::service::store::ManifestStore`] writes,
//! and `EventReceiver` event dispatch — to drive a flat
//! `FuturesUnordered` over [`crate::service::manifest::fetch_full_manifest`]
//! plus a synchronous transitive walk. The warm
//! [`crate::service::cache::MemoryCache`] it leaves behind makes the
//! subsequent BFS phase a pure cache-hit walk: no network, no rayon
//! re-parse hop on `extract_core_version`.
//!
//! Intended for the lockfile-only path (`utoo deps`) which has no
//! pipeline consumer for `BuildEvent::PackageResolved` — install paths
//! still go through [`super::preload::preload_manifests`] so the
//! pipeline keeps its early-start signal.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};

use crate::model::manifest::CoreVersionManifest;
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

/// Collect dependencies from any deps map, filtering out non-registry specs.
fn collect_deps(map: Option<&std::collections::HashMap<String, String>>) -> Vec<Dep> {
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

/// Resolve `(name, spec)` against the cached `FullManifest` synchronously.
///
/// Inlines the work that `UnifiedRegistry::resolve_via_full_manifest` does
/// after a cache hit — pick a version, parse just that subset, populate
/// the per-version cache slot the BFS phase will read from. Skips the
/// rayon/`spawn_blocking` hop because the caller is already doing
/// CPU-bound bookkeeping between fetches.
fn settle_spec(name: &str, spec: &str, cache: &MemoryCache, peer_deps: PeerDeps) -> Vec<Dep> {
    let Some(full) = cache.get_full_manifest(name) else {
        return Vec::new();
    };
    let Ok(resolved_version) = resolve_target_version((&*full).into(), spec) else {
        return Vec::new();
    };
    if let Some(cached) = cache.get_version_manifest(name, &resolved_version) {
        return extract_transitive_deps(&cached, peer_deps);
    }
    let Some(core) = full.get_core_version(&resolved_version) else {
        return Vec::new();
    };
    let core_arc = Arc::new(core);
    cache.set_version_manifest(name.to_string(), resolved_version, Arc::clone(&core_arc));
    extract_transitive_deps(&core_arc, peer_deps)
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
    // sync resolutions from re-walking the same transitive subtree.
    let mut seen_specs: HashSet<(String, String)> = HashSet::new();
    // Names whose full manifest is either cached or in flight. Spec-level
    // dedup happens in `seen_specs` above; this set is the gate that
    // prevents two concurrent fetches for the same package (sibling
    // specs queue against the in-flight one rather than racing).
    let mut fetched_names: HashSet<String> = HashSet::new();
    // Specs that arrived while their package's full manifest was still
    // in flight — we'll settle them once the fetch lands.
    let mut deferred_specs: Vec<(String, String)> = Vec::new();
    let mut futs = FuturesUnordered::new();
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

            // Full manifest already cached: skip the network round-trip,
            // settle synchronously and queue this package's transitive
            // deps. This is the hot path on the second-and-later spec
            // for any popular package (lodash, semver, etc.).
            if cache.get_full_manifest(&name).is_some() {
                let new_deps = settle_spec(&name, &spec, cache, peer_deps);
                pending.extend(new_deps);
                continue;
            }

            // Fetch in flight for this name — defer settling this spec
            // until the fetch lands. The deferred set is small (only
            // sibling specs for in-flight names) so the linear scan is
            // cheaper than another HashMap.
            if !fetched_names.insert(name.clone()) {
                deferred_specs.push((name, spec));
                continue;
            }

            let registry_url = registry_url.to_string();
            let n = name.clone();
            futs.push(async move {
                let start = tokio::time::Instant::now();
                let result = fetch_full_manifest(FetchManifestOptions {
                    registry_url: &registry_url,
                    name: &n,
                    format: MetadataFormat::Abbreviated,
                    etag: None,
                })
                .await;
                let elapsed_ms = start.elapsed().as_millis() as u64;
                (name, spec, result, elapsed_ms)
            });
        }

        if futs.is_empty() {
            break;
        }

        let Some((name, spec, result, elapsed_ms)) = futs.next().await else {
            break;
        };

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
                cache.set_full_manifest(name.clone(), Arc::new(manifest));

                let new_deps = settle_spec(&name, &spec, cache, peer_deps);
                pending.extend(new_deps);

                // Drain any sibling specs that arrived while this fetch
                // was in flight. `extract_if`-style retain in place.
                let mut i = 0;
                while i < deferred_specs.len() {
                    if deferred_specs[i].0 == name {
                        let (n, s) = deferred_specs.swap_remove(i);
                        let new_deps = settle_spec(&n, &s, cache, peer_deps);
                        pending.extend(new_deps);
                    } else {
                        i += 1;
                    }
                }
            }
            Ok(FetchManifestResult::NotModified) => {
                // No ETag was sent on these requests, so 304 is unreachable
                // here in practice; treat it as a soft-failure to keep the
                // path total.
                stats.failed_count += 1;
            }
            Err(e) => {
                stats.failed_count += 1;
                tracing::debug!("fast_preload failed for {}: {}", name, e);
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
