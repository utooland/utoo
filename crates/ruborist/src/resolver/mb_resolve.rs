//! Two-phase manifest fetcher: phase 1 pure HTTP (mirrors
//! `manifest-bench` standalone exactly), phase 2 rayon batch parse +
//! settle.
//!
//! ## Phase split
//!
//! Per-fetch parse work was the real bottleneck in v1/v2 — `simd_json`
//! ran in `spawn_blocking` threads that competed with tokio runtime
//! workers for CPU on the 2-core GHA box. When 50+ parses ran in
//! parallel, tokio workers couldn't drive sockets, so `eff_parallel`
//! capped at ~47 against the 96 cap (vs `manifest-bench` standalone's
//! 75 on the same box).
//!
//! v3 separates the work:
//!
//! - **Phase 1** — `mb_style_pure_fetch` is a structural copy of
//!   `manifest-bench`'s main loop: `spawn_one` (GET + body recv,
//!   nothing else) + 1-for-1 refill on completion. The future body
//!   has zero CPU work, so the tokio runtime workers retain full CPU
//!   to drive sockets and `eff_parallel` reaches the same level as
//!   the standalone bench.
//!
//! - **Phase 2** — bulk parse on rayon (off the tokio runtime). For
//!   each fetched body: parse `FullManifest` envelope, resolve every
//!   spec we need for this name, materialize `CoreVersionManifest`
//!   subtrees, populate cache slots, collect transitive deps for the
//!   next iteration.
//!
//! Phases alternate until `pending` is empty (typical project: 3-5
//! iterations as transitive deps fan out wave by wave).
//!
//! Phase 1 is the line we measure against `manifest-bench` —
//! `p1-breakdown mb_fetch_iter=N phase1_http_wall=...` traces let us
//! check eff_parallel directly.
//!
//! Wired in via `UTOO_RESOLVE=mb` env var (see
//! `pm::helper::ruborist_context::Context::build_deps`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use rayon::prelude::*;
use serde::Deserialize;

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::PeerDeps;
use crate::resolver::preload::{Dep, PreloadConfig};
use crate::resolver::version::resolve_target_version;
use crate::service::MemoryCache;
use crate::service::http::get_client;
use crate::spec::SpecStr;

#[derive(Debug, Default)]
pub struct MbFetchStats {
    pub success: usize,
    pub fail: usize,
    pub iterations: usize,
}

/// Phase 1 result: one body per fetched name. `bytes` is `None` on
/// transport / non-2xx — kept in the result vector so phase 2 can
/// account for it, but contributes no settle work.
struct FetchOutcome {
    name: String,
    bytes: Option<Bytes>,
}

/// Phase 2 per-name output. `full` is `None` on parse failure.
struct ParseOutcome {
    name: String,
    full: Option<Arc<FullManifest>>,
    /// Per-spec settled subtrees: `(spec, resolved_version, core)`.
    /// Empty when the body failed to fetch / parse, or when no spec
    /// resolves against the manifest.
    settled: Vec<(String, String, Arc<CoreVersionManifest>)>,
    /// Transitive deps collected across all settled subtrees for this
    /// name. Already filtered to registry specs; the main loop dedups
    /// against `done_names` before queueing.
    transitives: Vec<Dep>,
}

fn collect_deps(map: Option<&HashMap<String, String>>) -> Vec<Dep> {
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

/// Phase 1 — structural copy of `manifest-bench`'s main loop. Future
/// body does ONLY GET + body recv; no parse, no cache writes, no
/// dedup. Returns one `FetchOutcome` per input name in arrival order.
async fn mb_style_pure_fetch(
    names: Vec<String>,
    registry_url: &str,
    concurrency: usize,
) -> Vec<FetchOutcome> {
    let client = match get_client() {
        Ok(c) => c.clone(),
        Err(e) => {
            tracing::warn!("get_client failed: {e}");
            return Vec::new();
        }
    };

    let mut results: Vec<FetchOutcome> = Vec::with_capacity(names.len());
    let mut futs = FuturesUnordered::new();
    let mut idx = 0usize;

    let spawn_one = |client: &reqwest::Client,
                     registry_url: &str,
                     name: String,
                     futs: &mut FuturesUnordered<_>| {
        let url = format!("{}/{}", registry_url, name);
        let client = client.clone();
        futs.push(Box::pin(async move {
            let bytes = match client
                .get(&url)
                .header("accept", "application/vnd.npm.install-v1+json")
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => resp.bytes().await.ok(),
                _ => None,
            };
            FetchOutcome { name, bytes }
        }));
    };

    while idx < names.len() && futs.len() < concurrency {
        spawn_one(&client, registry_url, names[idx].clone(), &mut futs);
        idx += 1;
    }

    while let Some(outcome) = futs.next().await {
        results.push(outcome);
        if idx < names.len() {
            spawn_one(&client, registry_url, names[idx].clone(), &mut futs);
            idx += 1;
        }
    }

    results
}

/// Sync phase 2 worker: parse one body, settle all specs we need for
/// this name. Runs on rayon (called from `par_iter` in
/// `parse_settle_batch`).
fn parse_one_body(
    name: String,
    raw: Bytes,
    specs: Vec<String>,
    peer_deps: PeerDeps,
) -> ParseOutcome {
    use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};

    let raw_arc: Arc<[u8]> = Arc::from(raw.as_ref());
    let mut buf = raw.to_vec();
    let parsed = match simd_json::to_borrowed_value(&mut buf) {
        Ok(v) => v,
        Err(_) => {
            return ParseOutcome {
                name,
                full: None,
                settled: Vec::new(),
                transitives: Vec::new(),
            };
        }
    };

    let envelope_name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| name.clone());
    let dist_tags: HashMap<String, String> = parsed
        .get("dist-tags")
        .and_then(|v| HashMap::<String, String>::deserialize(v).ok())
        .unwrap_or_default();
    let versions_keys: Vec<String> = parsed
        .get("versions")
        .and_then(simd_json::prelude::ValueAsObject::as_object)
        .map(|obj| obj.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();

    let full = FullManifest {
        name: envelope_name,
        dist_tags,
        versions: versions_keys,
        raw: Arc::clone(&raw_arc),
        ..Default::default()
    };
    let full_arc = Arc::new(full);

    // For each requested spec, resolve + extract version subtree.
    // Cache the per-(name, version) `CoreVersionManifest` so sibling
    // specs that resolve to the same version reuse the same Arc.
    let mut version_cache: HashMap<String, Arc<CoreVersionManifest>> = HashMap::new();
    let mut settled = Vec::with_capacity(specs.len());
    let mut transitives = Vec::new();

    for spec in specs {
        let Ok(resolved_version) = resolve_target_version((&*full_arc).into(), &spec) else {
            continue;
        };
        let core_arc = if let Some(cached) = version_cache.get(&resolved_version) {
            Arc::clone(cached)
        } else {
            let Some(core) = parsed
                .get("versions")
                .and_then(|v| v.get(resolved_version.as_str()))
                .and_then(|version_obj| CoreVersionManifest::deserialize(version_obj).ok())
            else {
                continue;
            };
            let arc = Arc::new(core);
            version_cache.insert(resolved_version.clone(), Arc::clone(&arc));
            arc
        };
        transitives.extend(extract_transitive(&core_arc, peer_deps));
        settled.push((spec, resolved_version, core_arc));
    }

    ParseOutcome {
        name,
        full: Some(full_arc),
        settled,
        transitives,
    }
}

/// Phase 2 dispatcher: hands the batch to rayon, awaits the result.
async fn parse_settle_batch(
    bodies: Vec<FetchOutcome>,
    by_name: HashMap<String, Vec<String>>,
    peer_deps: PeerDeps,
) -> Vec<ParseOutcome> {
    let work: Vec<(String, Bytes, Vec<String>)> = bodies
        .into_iter()
        .filter_map(|f| {
            let bytes = f.bytes?;
            let specs = by_name.get(&f.name).cloned().unwrap_or_default();
            Some((f.name, bytes, specs))
        })
        .collect();

    if work.is_empty() {
        return Vec::new();
    }

    tokio::task::spawn_blocking(move || {
        work.into_par_iter()
            .map(|(name, raw, specs)| parse_one_body(name, raw, specs, peer_deps))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

/// Two-phase mb-style fetch with rayon batch parse. See module docs.
pub async fn mb_fetch(
    initial_deps: Vec<Dep>,
    registry_url: &str,
    cache: &MemoryCache,
    config: &PreloadConfig,
) -> MbFetchStats {
    let mut stats = MbFetchStats::default();
    let mut pending_specs: Vec<Dep> = initial_deps;
    // (name, spec) pairs we've already processed (settled or queued
    // to settle). Without this, sibling-settle's transitive deps can
    // re-introduce already-walked specs and the outer loop never
    // terminates — peer / optional dep cycles trivially trigger this.
    let mut seen_specs: HashSet<(String, String)> = HashSet::new();
    let mut done_names: HashSet<String> = HashSet::new();
    let conc = config.concurrency;
    let peer_deps = config.peer_deps;
    let total_start = tokio::time::Instant::now();

    // Filter the initial seed through `seen_specs` too — root + workspace
    // edges can list the same dep multiple times across workspaces.
    pending_specs.retain(|(n, s)| seen_specs.insert((n.clone(), s.clone())));

    while !pending_specs.is_empty() {
        stats.iterations += 1;
        let iter = stats.iterations;

        // Group this iteration's pending specs by name.
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for (name, spec) in pending_specs.drain(..) {
            by_name.entry(name).or_default().push(spec);
        }

        // Names whose full manifest is already cached from a prior
        // iteration: settle their siblings synchronously (cheap
        // semver match + cache lookup; no parse if version_manifest
        // already cached, otherwise quick simd_json subtree extract).
        let mut sibling_only: Vec<(String, Vec<String>)> = Vec::new();
        let mut to_fetch: Vec<String> = Vec::with_capacity(by_name.len());
        for (name, specs) in &by_name {
            if done_names.contains(name) {
                sibling_only.push((name.clone(), specs.clone()));
            } else {
                to_fetch.push(name.clone());
            }
        }

        // Sibling settles (rare on real workloads — most names appear
        // exactly once across the whole walk). New transitives go
        // through `seen_specs` dedup before joining `pending_specs`.
        for (name, specs) in sibling_only {
            let Some(full) = cache.get_full_manifest(&name) else {
                continue;
            };
            for spec in specs {
                let Ok(resolved) = resolve_target_version((&*full).into(), &spec) else {
                    continue;
                };
                let new_deps = if let Some(cached) = cache.get_version_manifest(&name, &resolved) {
                    cache.set_version_manifest(name.clone(), spec.clone(), Arc::clone(&cached));
                    extract_transitive(&cached, peer_deps)
                } else if let Some(core) = full.get_core_version(&resolved) {
                    let core_arc = Arc::new(core);
                    cache.set_version_manifest(name.clone(), spec.clone(), Arc::clone(&core_arc));
                    cache.set_version_manifest(name.clone(), resolved, Arc::clone(&core_arc));
                    extract_transitive(&core_arc, peer_deps)
                } else {
                    Vec::new()
                };
                pending_specs.extend(
                    new_deps
                        .into_iter()
                        .filter(|(n, s)| seen_specs.insert((n.clone(), s.clone()))),
                );
            }
        }

        if to_fetch.is_empty() {
            // Iteration drained pending entirely via sibling settles.
            continue;
        }

        // PHASE 1 — pure HTTP, mb-style.
        let p1_start = tokio::time::Instant::now();
        let bodies = mb_style_pure_fetch(to_fetch.clone(), registry_url, conc).await;
        let p1_wall = p1_start.elapsed().as_millis();
        let total_bytes: usize = bodies
            .iter()
            .map(|b| b.bytes.as_ref().map(|v| v.len()).unwrap_or(0))
            .sum();
        tracing::info!(
            "p1-breakdown mb_fetch iter={} phase1_http_wall={}ms n={} bytes={}",
            iter,
            p1_wall,
            to_fetch.len(),
            total_bytes,
        );

        // PHASE 2 — rayon batch parse + settle.
        let p2_start = tokio::time::Instant::now();
        let by_name_for_parse = by_name
            .iter()
            .filter(|(name, _)| !done_names.contains(*name))
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect::<HashMap<_, _>>();
        let parsed = parse_settle_batch(bodies, by_name_for_parse, peer_deps).await;
        let p2_wall = p2_start.elapsed().as_millis();

        let mut new_transitives: Vec<Dep> = Vec::new();
        let mut settle_count = 0usize;
        let mut fail_count = 0usize;
        for outcome in parsed {
            done_names.insert(outcome.name.clone());
            let Some(full_arc) = outcome.full else {
                fail_count += 1;
                continue;
            };
            cache.set_full_manifest(outcome.name.clone(), Arc::clone(&full_arc));
            for (spec, resolved, core) in outcome.settled {
                cache.set_version_manifest(outcome.name.clone(), spec, Arc::clone(&core));
                cache.set_version_manifest(outcome.name.clone(), resolved, Arc::clone(&core));
                settle_count += 1;
            }
            new_transitives.extend(outcome.transitives);
        }
        // Names that fetched but failed parse — still mark done so we
        // don't refetch them next iteration.
        for name in to_fetch {
            done_names.insert(name);
        }

        stats.success += settle_count;
        stats.fail += fail_count;

        let new_unique: Vec<Dep> = new_transitives
            .into_iter()
            .filter(|(n, s)| seen_specs.insert((n.clone(), s.clone())))
            .collect();

        tracing::info!(
            "p1-breakdown mb_fetch iter={} phase2_parse_wall={}ms settles={} fail={} new_unique={}",
            iter,
            p2_wall,
            settle_count,
            fail_count,
            new_unique.len(),
        );

        pending_specs.extend(new_unique);
    }

    let total_wall = total_start.elapsed().as_millis();
    tracing::info!(
        "p1-breakdown mb_fetch total_wall={}ms iters={} settled={} fail={}",
        total_wall,
        stats.iterations,
        stats.success,
        stats.fail,
    );

    stats
}
