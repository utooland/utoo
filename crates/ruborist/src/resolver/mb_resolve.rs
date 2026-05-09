//! Standalone manifest preload for the lockfile-only path.
//!
//! Mirrors `crates/preload-bench`'s loop shape verbatim, but lives
//! inside ruborist so it can populate `MemoryCache` for the BFS phase
//! to read. Used by `service::api::build_deps` whenever the caller
//! has `skip_preload=true` and no warm project cache — i.e. the
//! `utoo deps` (lockfile-only) path.
//!
//! Bypasses every other ruborist service layer:
//!   * `service::http::get_client` — own `reqwest::Client` built per
//!     call, no global LazyLock, no `dns_resolver(shared_resolver)`,
//!     no `connect_timeout`, `pool_max_idle_per_host(256)` matching
//!     `preload-bench` / `manifest-bench`.
//!   * `service::manifest::fetch_full_manifest_with_settle` — own
//!     `reqwest::get + body.bytes() + spawn_blocking(simd_json
//!     to_borrowed_value)`, no `RetryIf`, no `FETCH_TIMINGS`.
//!   * `service::registry::UnifiedRegistry` — no `OnceMap` inflight
//!     gates, no `ManifestStore`, no `EventReceiver`.
//!
//! The only `service::*` touched is `MemoryCache::set_full_manifest`
//! and `MemoryCache::set_version_manifest` — thin DashMap wrappers
//! the BFS phase reads from. Without that, BFS would have nothing to
//! resolve against.
//!
//! Why a separate path: same-run CI data shows `preload-bench`
//! (self-contained, transitive walk, 4153 fetches) lands at ~2.57s
//! while ruborist's existing `fast_preload` path (combined parse via
//! service layers, 2733 fetches) lands at ~2.67s on the same network
//! — so on a per-fetch basis the service-layer path is ~50 % slower.
//! Removing the layers should close that gap.

use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use futures::stream::{FuturesUnordered, StreamExt};
use parking_lot::Mutex;
use petgraph::graph::{EdgeIndex, NodeIndex};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::model::graph::DependencyGraph;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::PeerDeps;
use crate::resolver::builder::{
    BuildDepsConfig, ProcessResult, collect_unresolved_edges, process_dependency_with_resolved,
};
use crate::resolver::preload::{Dep, PreloadConfig};
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::service::MemoryCache;
use crate::spec::SpecStr;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::ResolvedPackage;

#[derive(Debug, Default)]
pub struct MbFetchStats {
    pub success: usize,
    pub fail: usize,
}

/// Build a fresh `reqwest::Client` matching `preload-bench` /
/// `manifest-bench` exactly: aws-lc-rs TLS provider via
/// `use_preconfigured_tls`, `pool_max_idle_per_host(256)`, no
/// proxy, `http1_only`. The reqwest crate's
/// `rustls-tls-native-roots` feature on Linux still bundles ring
/// for `service::http`'s global client, but this client overrides
/// at construction time — both providers coexist in the binary.
#[cfg(not(target_arch = "wasm32"))]
fn build_mb_client() -> Result<reqwest::Client> {
    // Idempotent: first install_default wins; subsequent calls are
    // no-ops. Sets the process-wide default for any rustls consumer
    // that builds a `ClientConfig` without explicit provider.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        // Tolerate individual bad roots — same defensive load pattern
        // as `service::http::build_rustls_config`.
        let _ = roots.add(cert);
    }

    let tls_config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| anyhow!("rustls protocol versions: {e}"))?
    .with_root_certificates(roots)
    .with_no_client_auth();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .no_proxy()
        .pool_max_idle_per_host(256)
        .http1_only()
        .build()
        .context("build reqwest client for mb_resolve")
}

#[cfg(target_arch = "wasm32")]
fn build_mb_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .context("build reqwest client for mb_resolve")
}

/// Collect deps from a deps map, filtering non-registry specs.
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

/// What a future returns when it lands. The main loop uses
/// `transitives` to extend `pending`, plus the cache writes already
/// happened inside the future. Only `fetched=true` futures populate
/// `body_cache` and trigger sibling drain.
struct FetchOutcome {
    /// The dep key (alias name as it appears in the parent's deps map).
    /// Used by `graph_worker` to filter `edge_targets`, which is keyed
    /// on the alias.
    name: String,
    /// The real package name after npm-alias normalization (e.g.
    /// `name="ms"` + `spec="npm:raw-body@2.1.3"` → `real_name="raw-body"`).
    /// Used by the main loop for `body_cache` / `deferred_by_name` /
    /// `in_flight_names` keying, so two distinct aliases pointing at
    /// the same package share dedup.
    real_name: String,
    /// The spec that triggered this fetch / settle. Used by the
    /// main loop to look up the cached `CoreVersionManifest` for
    /// `PackageResolved` event emission (the future already wrote
    /// `(name, primary_spec)` to the cache).
    primary_spec: String,
    transitives: Vec<Dep>,
    fetched: bool,
    /// Per-future wall (network + body recv + spawn_blocking parse).
    /// Summed across all futures, divided by mb_fetch total wall =
    /// eff_parallel — the same number `manifest-bench` reports as
    /// `avg_conc`. Used to spot wave-shape underutilization.
    wall_us: u64,
    /// Per-future network-only wall (request.send + body.bytes).
    /// `wall_us - net_us` is the spawn_blocking parse contribution.
    net_us: u64,
}

type Fut = Pin<Box<dyn std::future::Future<Output = FetchOutcome> + Send>>;

/// `(name, spec) → (FullManifest, resolved_version, version_subtree, transitive_deps)`.
type ParseResult = (
    Arc<FullManifest>,
    String,
    Arc<CoreVersionManifest>,
    Vec<Dep>,
);

/// Single combined parse: one `simd_json::to_borrowed_value` over the
/// raw body extracts the envelope (name, dist-tags, versions keys)
/// AND deserializes the resolved version's `CoreVersionManifest`
/// subtree. Same shape as the parse step in `preload-bench`.
fn parse_combined(raw: Arc<[u8]>, spec: &str, peer_deps: PeerDeps) -> Option<ParseResult> {
    use simd_json::prelude::{ValueAsObject, ValueAsScalar, ValueObjectAccess};

    let mut buf = (*raw).to_vec();
    let parsed = simd_json::to_borrowed_value(&mut buf).ok()?;

    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let dist_tags: HashMap<String, String> = parsed
        .get("dist-tags")
        .and_then(|v| HashMap::<String, String>::deserialize(v).ok())
        .unwrap_or_default();
    let versions_keys: Vec<String> = parsed
        .get("versions")
        .and_then(ValueAsObject::as_object)
        .map(|obj| obj.keys().map(|k| k.to_string()).collect())
        .unwrap_or_default();

    let full = FullManifest {
        name,
        dist_tags,
        versions: versions_keys,
        raw: Arc::clone(&raw),
        ..Default::default()
    };

    let resolved = resolve_target_version((&full).into(), spec).ok()?;
    let core = parsed
        .get("versions")
        .and_then(|v| v.get(resolved.as_str()))
        .and_then(|version_obj| CoreVersionManifest::deserialize(version_obj).ok())?;
    let core_arc = Arc::new(core);
    let transitives = extract_transitive(&core_arc, peer_deps);

    Some((Arc::new(full), resolved, core_arc, transitives))
}

/// Fetch + combined parse + cache write for one `(name, spec)`.
/// Future body owns all per-fetch work; main loop only extends
/// `pending` from the returned transitives and refills `futs`.
fn spawn_fetch(
    client: reqwest::Client,
    registry_url: Arc<String>,
    name: String,
    spec: String,
    cache: MemoryCache,
    body_cache: Arc<Mutex<HashMap<String, Arc<[u8]>>>>,
    peer_deps: PeerDeps,
) -> Fut {
    Box::pin(async move {
        let fut_start = Instant::now();
        let primary_spec = spec.clone();
        // Normalize npm-alias / workspace specs so the registry hit
        // and the manifest parse run against the *real* package, not
        // the alias name. Cache writes still go under the original
        // (alias_name, alias_spec) key so `graph_worker` can locate
        // them via `edge_targets`.
        let (real_name, real_spec) = normalize_spec(&name, &spec);
        let url = format!("{}/{}", registry_url, real_name);
        let resp = match client
            .get(&url)
            .header("accept", "application/vnd.npm.install-v1+json")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => {
                let wall_us = fut_start.elapsed().as_micros() as u64;
                return FetchOutcome {
                    name,
                    real_name,
                    primary_spec,
                    transitives: Vec::new(),
                    fetched: true,
                    wall_us,
                    net_us: wall_us,
                };
            }
        };
        let raw_bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(_) => {
                let wall_us = fut_start.elapsed().as_micros() as u64;
                return FetchOutcome {
                    name,
                    real_name,
                    primary_spec,
                    transitives: Vec::new(),
                    fetched: true,
                    wall_us,
                    net_us: wall_us,
                };
            }
        };
        let net_us = fut_start.elapsed().as_micros() as u64;
        let raw_arc: Arc<[u8]> = Arc::from(raw_bytes.as_ref());
        // Body cache is keyed by real_name so two aliases pointing at
        // the same registry package share the body and only one fetch
        // fires. Sibling drains know to use real_name (see
        // `deferred_by_name` keying in the main loop).
        body_cache
            .lock()
            .insert(real_name.clone(), Arc::clone(&raw_arc));

        let real_spec_for_parse = real_spec.clone();
        let peer = peer_deps;
        let parsed = tokio::task::spawn_blocking(move || {
            parse_combined(raw_arc, &real_spec_for_parse, peer)
        })
        .await
        .ok()
        .flatten();

        let transitives = match parsed {
            Some((full_arc, resolved, core_arc, transitives)) => {
                cache.set_full_manifest(real_name.clone(), Arc::clone(&full_arc));
                // Under the alias key so `graph_worker` finds it.
                cache.set_version_manifest(name.clone(), spec, Arc::clone(&core_arc));
                // Under the real key so subsequent direct deps on
                // the same package@version dedupe correctly.
                cache.set_version_manifest(real_name.clone(), resolved, core_arc);
                transitives
            }
            None => Vec::new(),
        };

        let wall_us = fut_start.elapsed().as_micros() as u64;
        FetchOutcome {
            name,
            real_name,
            primary_spec,
            transitives,
            fetched: true,
            wall_us,
            net_us,
        }
    })
}

/// Settle-only future for a sibling spec whose `(name)` body already
/// landed via a sibling fetch. Same combined parse, no network.
fn spawn_settle(
    name: String,
    spec: String,
    raw: Arc<[u8]>,
    cache: MemoryCache,
    peer_deps: PeerDeps,
) -> Fut {
    Box::pin(async move {
        let fut_start = Instant::now();
        let primary_spec = spec.clone();
        let (real_name, real_spec) = normalize_spec(&name, &spec);
        let real_spec_for_parse = real_spec.clone();
        let peer = peer_deps;
        let parsed = tokio::task::spawn_blocking(move || {
            parse_combined(Arc::clone(&raw), &real_spec_for_parse, peer)
        })
        .await
        .ok()
        .flatten();

        let transitives = match parsed {
            Some((full_arc, resolved, core_arc, transitives)) => {
                // Don't overwrite full_manifest — the original fetcher
                // already set it under real_name. Populate version
                // slots so BFS hits the (alias_name, alias_spec)
                // early-return.
                cache.set_full_manifest(real_name.clone(), full_arc);
                cache.set_version_manifest(name.clone(), spec, Arc::clone(&core_arc));
                cache.set_version_manifest(real_name.clone(), resolved, core_arc);
                transitives
            }
            None => Vec::new(),
        };

        let wall_us = fut_start.elapsed().as_micros() as u64;
        FetchOutcome {
            name,
            real_name,
            primary_spec,
            transitives,
            fetched: false,
            wall_us,
            // Settle-only futures have no network component.
            net_us: 0,
        }
    })
}

/// Streaming preload with transitive walk. Self-contained — no
/// dependency on `service::http` / `service::manifest` /
/// `service::registry` beyond `MemoryCache` writes.
pub async fn mb_fetch(
    initial_deps: Vec<Dep>,
    registry_url: &str,
    cache: &MemoryCache,
    config: &PreloadConfig,
) -> MbFetchStats {
    let mut stats = MbFetchStats::default();
    // Per-future wall + net sums for eff_parallel computation.
    // sum_wall_us / total_wall_ms / 1000 = eff_parallel for the
    // whole future-body span (network + parse + cache writes).
    // sum_net_us / total_wall_ms / 1000 = network-only eff_parallel,
    // directly comparable to manifest-bench's avg_conc.
    let mut sum_wall_us: u64 = 0;
    let mut sum_net_us: u64 = 0;
    let mut fetch_count: u64 = 0;
    let mut settle_count: u64 = 0;
    let total_start = Instant::now();

    let client = match build_mb_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("mb_resolve client build failed: {e}");
            return stats;
        }
    };
    let registry = Arc::new(registry_url.trim_end_matches('/').to_string());
    let cap = config.concurrency;
    let peer_deps = config.peer_deps;

    // Spec-level dedup across the entire run.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut pending: VecDeque<Dep> = VecDeque::new();
    for (name, spec) in initial_deps {
        if seen.insert((name.clone(), spec.clone())) {
            pending.push_back((name, spec));
        }
    }

    // Sibling-fetch dedup: when two specs for the same package are
    // both in flight, only the first fires a fetch; the second
    // arrives at the cached body and goes through `spawn_settle`.
    // Keyed by *real* package name (post npm-alias normalization)
    // so two distinct aliases pointing at the same registry package
    // share dedup.
    let body_cache: Arc<Mutex<HashMap<String, Arc<[u8]>>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut in_flight_real_names: HashSet<String> = HashSet::new();
    let mut deferred_by_real_name: HashMap<String, Vec<(String, String)>> = HashMap::new();

    let mut futs: FuturesUnordered<Fut> = FuturesUnordered::new();

    loop {
        // Refill to cap.
        while futs.len() < cap {
            let Some((name, spec)) = pending.pop_front() else {
                break;
            };
            let (real_name, _) = normalize_spec(&name, &spec);
            // Sibling fast path: body already cached.
            if let Some(raw) = body_cache.lock().get(&real_name).cloned() {
                futs.push(spawn_settle(name, spec, raw, cache.clone(), peer_deps));
                continue;
            }
            // Defer if a fetch for this real package is already in flight.
            if !in_flight_real_names.insert(real_name.clone()) {
                deferred_by_real_name
                    .entry(real_name)
                    .or_default()
                    .push((name, spec));
                continue;
            }
            futs.push(spawn_fetch(
                client.clone(),
                Arc::clone(&registry),
                name,
                spec,
                cache.clone(),
                Arc::clone(&body_cache),
                peer_deps,
            ));
        }

        if futs.is_empty() {
            break;
        }

        let Some(out) = futs.next().await else { break };

        sum_wall_us += out.wall_us;
        sum_net_us += out.net_us;
        if out.fetched {
            fetch_count += 1;
        } else {
            settle_count += 1;
        }

        if out.transitives.is_empty() && out.fetched {
            // Empty result from a fetch is ambiguous (no transitives
            // OR a fetch/parse failure). Track conservatively as
            // success — the FETCH_TIMINGS-equivalent counter is
            // omitted in this path on purpose to keep the future
            // body lean.
            stats.success += 1;
        } else if out.fetched {
            stats.success += 1;
        }

        // Drain sibling specs deferred while the fetch was in flight.
        if out.fetched
            && let Some(siblings) = deferred_by_real_name.remove(&out.real_name)
            && let Some(raw) = body_cache.lock().get(&out.real_name).cloned()
        {
            for (sibling_name, sibling_spec) in siblings {
                futs.push(spawn_settle(
                    sibling_name,
                    sibling_spec,
                    Arc::clone(&raw),
                    cache.clone(),
                    peer_deps,
                ));
            }
        }

        // Extend pending with new transitive specs, dedup.
        for (name, spec) in out.transitives {
            if seen.insert((name.clone(), spec.clone())) {
                pending.push_back((name, spec));
            }
        }
    }

    let total_wall_ms = total_start.elapsed().as_millis();
    let total_wall_us = (total_wall_ms as u64).saturating_mul(1000);
    let eff_par_full = if total_wall_us > 0 {
        sum_wall_us as f64 / total_wall_us as f64
    } else {
        0.0
    };
    let eff_par_net = if total_wall_us > 0 {
        sum_net_us as f64 / total_wall_us as f64
    } else {
        0.0
    };
    let avg_wall_us = sum_wall_us
        .checked_div(fetch_count + settle_count)
        .unwrap_or(0);
    let avg_net_us = sum_net_us.checked_div(fetch_count).unwrap_or(0);
    tracing::info!(
        "p1-breakdown mb_fetch wall={}ms ok={} fail={} fetch={} settle={} sum_wall={}ms sum_net={}ms avg_wall={}us avg_net={}us eff_par_full={:.1} eff_par_net={:.1}",
        total_wall_ms,
        stats.success,
        stats.fail,
        fetch_count,
        settle_count,
        sum_wall_us / 1000,
        sum_net_us / 1000,
        avg_wall_us,
        avg_net_us,
        eff_par_full,
        eff_par_net,
    );

    stats
}

// ============================================================================
// Folded streaming graph build — preload + BFS in one phase
// ============================================================================

/// Edges waiting on a `(name, spec)` fetch. Multiple parents can need
/// the same registry dep; we track them all and process inline as
/// soon as the manifest lands.
type EdgeTargets = HashMap<(String, String), Vec<(NodeIndex, EdgeIndex)>>;

/// Collect the unresolved registry edges from `node_idx` into
/// pending + edge_targets, dedup by spec via `seen_specs`.
/// Non-registry edges (workspace / git / http / file) are
/// deliberately left for the follow-up BFS sweep.
/// Process this node's unresolved registry edges:
/// * If the (name, spec) is already cached (a sibling subtree
///   resolved it earlier), call `process_dependency_with_resolved`
///   inline now. Newly-created child nodes recurse via this same
///   function so their edges are also enqueued/processed.
/// * Otherwise, register the (parent, edge_id) under `edge_targets`
///   so the eventual fetch result drains it; push to `pending` if
///   this `(name, spec)` hasn't been seen.
///
/// Without the inline-process path, `(name, spec)` keys added
/// AFTER their fetch already landed would never be drained — they'd
/// sit in `edge_targets` and the corresponding parent edges would
/// stay unresolved. CI run c02bb152 showed ~580 such orphans.
fn enqueue_node_edges(
    graph: &mut DependencyGraph,
    node_idx: NodeIndex,
    pending: &mut VecDeque<Dep>,
    seen_specs: &mut HashSet<(String, String)>,
    edge_targets: &mut EdgeTargets,
    cache: &MemoryCache,
    build_config: &BuildDepsConfig,
) {
    let mut work_stack: Vec<NodeIndex> = vec![node_idx];
    while let Some(idx) = work_stack.pop() {
        let edges = collect_unresolved_edges(graph, idx);
        for edge in edges {
            if !edge.spec.is_registry_spec() {
                continue;
            }
            let key = (edge.name.clone(), edge.spec.clone());

            // Cache-hit fast path: process immediately, no
            // edge_targets stash. Reuses the same process logic the
            // main loop uses on fetch result.
            if let Some(core_arc) = cache.get_version_manifest(&edge.name, &edge.spec) {
                let resolved = ResolvedPackage {
                    name: edge.name.clone(),
                    version: core_arc.version.clone(),
                    manifest: core_arc,
                };
                let edge_info = crate::resolver::edges::DependencyEdgeInfo {
                    edge_id: edge.edge_id,
                    name: edge.name.clone(),
                    spec: edge.spec.clone(),
                    edge_type: edge.edge_type,
                };
                if let ProcessResult::Created(new_idx) = process_dependency_with_resolved(
                    graph,
                    idx,
                    &edge_info,
                    &resolved,
                    build_config,
                ) {
                    work_stack.push(new_idx);
                }
                // Whether Created or Reused, this edge is now
                // resolved — don't queue.
                continue;
            }

            edge_targets
                .entry(key.clone())
                .or_default()
                .push((idx, edge.edge_id));
            if seen_specs.insert(key.clone()) {
                pending.push_back(key);
            }
        }
    }
}

/// Folded variant: combines `mb_fetch`'s streaming preload with the
/// graph mutations that BFS would otherwise do in a separate phase.
/// Each fetch result triggers inline `process_dependency_with_resolved`
/// for every parent edge waiting on `(name, spec)`. New nodes' edges
/// feed back into pending / edge_targets, so the walk continues
/// streaming-style without a separate level-by-level traversal.
///
/// CPU work (graph mutations) overlaps with network IO (more fetches
/// in flight via `FuturesUnordered`), so the 305 ms BFS phase
/// observed against a fully-warm cache is collapsed into mb_fetch's
/// wall instead of running serially after it.
///
/// Non-registry edges (workspace / git / http / file) and any edges
/// added after the streaming loop converges (override re-resolves
/// that diverge from the original spec) are left unresolved — the
/// caller must run a follow-up BFS sweep to handle them. For
/// `utoo deps` on registry-only workloads (the common case), the
/// sweep is a no-op.
/// One fetched/settled event, sent from main loop to graph worker.
/// The future already performed cache writes inline (cheap DashMap
/// inserts). Graph worker uses `cache.get_version_manifest` to
/// retrieve the manifest for `process_dependency_with_resolved`.
struct FetchEventMsg {
    name: String,
}

pub async fn mb_fetch_with_graph<R>(
    mut graph: DependencyGraph,
    registry_url: &str,
    cache: &MemoryCache,
    preload_config: &PreloadConfig,
    build_config: &BuildDepsConfig,
    receiver: Arc<R>,
) -> Result<(DependencyGraph, MbFetchStats)>
where
    R: EventReceiver + 'static,
{
    let mut stats = MbFetchStats::default();
    let total_start = Instant::now();

    let client = match build_mb_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("mb_resolve client build failed: {e}");
            return Ok((graph, stats));
        }
    };
    let registry = Arc::new(registry_url.trim_end_matches('/').to_string());
    let cap = preload_config.concurrency;
    let peer_deps = preload_config.peer_deps;

    // Initial seed: walk root + workspace nodes for unresolved
    // registry edges. Done inline before spawning workers (one-time
    // cost, not on the hot path).
    let mut seen_specs: HashSet<(String, String)> = HashSet::new();
    let mut pending: VecDeque<Dep> = VecDeque::new();
    let mut edge_targets: EdgeTargets = HashMap::new();

    let root_index = graph.root_index;
    enqueue_node_edges(
        &mut graph,
        root_index,
        &mut pending,
        &mut seen_specs,
        &mut edge_targets,
        cache,
        build_config,
    );
    let workspace_indices: Vec<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|&i| graph.get_node(i).is_some_and(|n| n.is_workspace()))
        .collect();
    for node_idx in workspace_indices {
        enqueue_node_edges(
            &mut graph,
            node_idx,
            &mut pending,
            &mut seen_specs,
            &mut edge_targets,
            cache,
            build_config,
        );
    }

    // Channels: main → graph (fetched events) + graph → main (new
    // pending specs). Bounded at 2 * cap so neither side stalls
    // waiting for the other under bursty wave behavior.
    let (fetch_tx, fetch_rx) = mpsc::channel::<FetchEventMsg>(cap * 2 + 16);
    let (specs_tx, mut specs_rx) = mpsc::channel::<Vec<Dep>>(cap * 2 + 16);

    // Spawn graph worker: owns the graph + edge_targets + seen_specs.
    // This task is CPU-only (no awaits except channel IO), so on a
    // multi-thread tokio runtime it gets its own worker thread,
    // freeing the main task's worker to drive socket polling. That
    // separation is the whole point of this rewrite — the inline
    // version observed zwin events 71 vs mb's 49, evidence of main
    // loop CPU starving the runtime's IO polling.
    let cache_clone = cache.clone();
    let build_config_owned = build_config.clone();
    let receiver_for_graph = Arc::clone(&receiver);
    let graph_handle = tokio::spawn(graph_worker(
        graph,
        edge_targets,
        seen_specs,
        cache_clone,
        build_config_owned,
        fetch_rx,
        specs_tx,
        receiver_for_graph,
    ));

    // Sibling-fetch dedup stays in main loop (drives FuturesUnordered).
    // Keyed by *real* package name (post npm-alias normalization)
    // so two distinct aliases pointing at the same registry package
    // share dedup; siblings store their alias `(name, spec)` so the
    // drain knows how to spawn `spawn_settle` with the right cache key.
    let body_cache: Arc<Mutex<HashMap<String, Arc<[u8]>>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut in_flight_real_names: HashSet<String> = HashSet::new();
    let mut deferred_by_real_name: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut futs: FuturesUnordered<Fut> = FuturesUnordered::new();

    let mut sum_wall_us: u64 = 0;
    let mut sum_net_us: u64 = 0;
    let mut fetch_count: u64 = 0;
    let mut settle_count: u64 = 0;
    // Number of FetchEventMsg sent to graph worker that haven't yet
    // had a corresponding Vec<Dep> response. Drives termination:
    // when futs empty + in_flight == 0, no more work pipelined.
    let mut in_flight_graph: usize = 0;

    loop {
        // Refill futs from pending up to cap.
        while futs.len() < cap {
            let Some((name, spec)) = pending.pop_front() else {
                break;
            };
            let (real_name, _) = normalize_spec(&name, &spec);
            if let Some(raw) = body_cache.lock().get(&real_name).cloned() {
                futs.push(spawn_settle(name, spec, raw, cache.clone(), peer_deps));
                continue;
            }
            if !in_flight_real_names.insert(real_name.clone()) {
                deferred_by_real_name
                    .entry(real_name)
                    .or_default()
                    .push((name, spec));
                continue;
            }
            futs.push(spawn_fetch(
                client.clone(),
                Arc::clone(&registry),
                name,
                spec,
                cache.clone(),
                Arc::clone(&body_cache),
                peer_deps,
            ));
        }

        // Termination: nothing in flight at fetch level AND graph
        // worker has nothing pending.
        if futs.is_empty() && in_flight_graph == 0 {
            break;
        }

        // Drive both halves: prefer draining specs back from graph
        // worker (unblocks new fetch dispatch) over starting another
        // fetch landing.
        tokio::select! {
            biased;
            maybe_specs = specs_rx.recv() => {
                match maybe_specs {
                    Some(specs) => {
                        pending.extend(specs);
                        in_flight_graph -= 1;
                    }
                    None => {
                        // Graph worker exited unexpectedly. Bail.
                        break;
                    }
                }
            }
            maybe_result = futs.next(), if !futs.is_empty() => {
                if let Some(out) = maybe_result {
                    sum_wall_us += out.wall_us;
                    sum_net_us += out.net_us;
                    if out.fetched {
                        fetch_count += 1;
                        stats.success += 1;
                    } else {
                        settle_count += 1;
                    }

                    // Pipeline early-start signal: emit
                    // PackageResolved as soon as the manifest is in
                    // cache. The install path's PipelineReceiver
                    // forwards this to the download worker so
                    // tarball download begins before BFS finishes.
                    // For lockfile-only callers (NoopReceiver), this
                    // is a no-op.
                    if let Some(core_arc) =
                        cache.get_version_manifest(&out.name, &out.primary_spec)
                    {
                        receiver.on_event(BuildEvent::PackageResolved(
                            (&*core_arc).into(),
                        ));
                    }

                    // Drain sibling specs deferred while the fetch
                    // was in flight. Sibling settles also produce a
                    // FetchEventMsg downstream.
                    if out.fetched
                        && let Some(siblings) = deferred_by_real_name.remove(&out.real_name)
                        && let Some(raw) = body_cache.lock().get(&out.real_name).cloned()
                    {
                        for (sibling_name, sibling_spec) in siblings {
                            futs.push(spawn_settle(
                                sibling_name,
                                sibling_spec,
                                Arc::clone(&raw),
                                cache.clone(),
                                peer_deps,
                            ));
                        }
                    }

                    // Send to graph worker. `send().await` only
                    // blocks if channel is full (cap * 2 buffer);
                    // under steady state shouldn't happen.
                    if fetch_tx.send(FetchEventMsg { name: out.name }).await.is_ok() {
                        in_flight_graph += 1;
                    }
                }
            }
        }
    }

    // Signal graph worker to exit, then await its finalization to
    // recover the graph + stats.
    drop(fetch_tx);
    let (graph, graph_stats) = graph_handle.await.context("graph worker join")??;

    let total_wall_ms = total_start.elapsed().as_millis();
    let total_wall_us = (total_wall_ms as u64).saturating_mul(1000);
    let eff_par_full = if total_wall_us > 0 {
        sum_wall_us as f64 / total_wall_us as f64
    } else {
        0.0
    };
    let eff_par_net = if total_wall_us > 0 {
        sum_net_us as f64 / total_wall_us as f64
    } else {
        0.0
    };
    let avg_net_us = sum_net_us.checked_div(fetch_count).unwrap_or(0);
    tracing::info!(
        "p1-breakdown mb_fetch_with_graph wall={}ms ok={} fetch={} settle={} sum_wall={}ms sum_net={}ms sum_graph={}ms avg_net={}us eff_par_full={:.1} eff_par_net={:.1} unresolved_targets={} graph_processed={} graph_new_specs={}",
        total_wall_ms,
        stats.success,
        fetch_count,
        settle_count,
        sum_wall_us / 1000,
        sum_net_us / 1000,
        graph_stats.sum_graph_us / 1000,
        avg_net_us,
        eff_par_full,
        eff_par_net,
        graph_stats.unresolved_remaining,
        graph_stats.processed,
        graph_stats.new_specs_emitted,
    );

    Ok((graph, stats))
}

#[derive(Debug, Default)]
struct GraphWorkerStats {
    sum_graph_us: u64,
    processed: usize,
    new_specs_emitted: usize,
    unresolved_remaining: usize,
}

/// CPU-only worker task that owns the graph + edge_targets +
/// seen_specs. Receives fetch events from main loop, mutates graph
/// via `process_dependency_with_resolved`, sends new pending specs
/// back. Designed to monopolize a tokio runtime worker thread so
/// the main loop's worker can drive socket polling without
/// competing for CPU.
#[allow(clippy::too_many_arguments)]
async fn graph_worker<R>(
    mut graph: DependencyGraph,
    mut edge_targets: EdgeTargets,
    mut seen_specs: HashSet<(String, String)>,
    cache: MemoryCache,
    build_config: BuildDepsConfig,
    mut fetch_rx: mpsc::Receiver<FetchEventMsg>,
    specs_tx: mpsc::Sender<Vec<Dep>>,
    receiver: Arc<R>,
) -> Result<(DependencyGraph, GraphWorkerStats)>
where
    R: EventReceiver + 'static,
{
    use crate::model::manifest::NodeManifest;
    let mut stats = GraphWorkerStats::default();

    while let Some(msg) = fetch_rx.recv().await {
        let graph_start = Instant::now();
        stats.processed += 1;

        // Drain edge_targets for every spec keyed under this name.
        // The fetch future already wrote both `(name, primary_spec)`
        // and `(name, resolved_version)` cache slots, so any
        // edge_targets entry for this name should hit cache.
        let primary_keys: Vec<(String, String)> = edge_targets
            .keys()
            .filter(|(n, _)| n == &msg.name)
            .cloned()
            .collect();

        let mut new_specs: Vec<Dep> = Vec::new();
        for (k_name, k_spec) in primary_keys {
            let Some(core_arc) = cache.get_version_manifest(&k_name, &k_spec) else {
                continue;
            };
            let resolved = ResolvedPackage {
                name: k_name.clone(),
                version: core_arc.version.clone(),
                manifest: core_arc,
            };
            let Some(targets) = edge_targets.remove(&(k_name.clone(), k_spec.clone())) else {
                continue;
            };
            for (parent_idx, edge_id) in targets {
                let edge_info = crate::resolver::edges::DependencyEdgeInfo {
                    edge_id,
                    name: k_name.clone(),
                    spec: k_spec.clone(),
                    edge_type: graph
                        .graph
                        .edge_weight(edge_id)
                        .and_then(|e| match e {
                            crate::model::graph::GraphEdge::Dependency(d) => Some(d.edge_type),
                            _ => None,
                        })
                        .unwrap_or(crate::model::node::EdgeType::Prod),
                };
                let result = process_dependency_with_resolved(
                    &mut graph,
                    parent_idx,
                    &edge_info,
                    &resolved,
                    &build_config,
                );
                if let ProcessResult::Created(new_idx) = result {
                    // Pipeline clone signal: emit PackagePlaced so
                    // the install path's clone worker can begin
                    // hardlinking from cache as soon as a node is
                    // placed in the graph. lockfile-only callers
                    // (NoopReceiver) drop this on the floor.
                    if let Some(node) = graph.get_node(new_idx)
                        && let NodeManifest::Registry(ref manifest) = node.manifest
                    {
                        let parent_path = graph.get_node(parent_idx).map(|p| p.path.as_path());
                        receiver.on_event(BuildEvent::PackagePlaced {
                            package: manifest.as_ref().into(),
                            path: &node.path,
                            parent_path,
                        });
                    }

                    // Walk the new node's edges. enqueue handles
                    // recursive cache-hit drain so already-cached
                    // specs get processed inline (still on this
                    // worker thread — graph mutations can't run on
                    // multiple threads with `&mut graph`).
                    enqueue_node_edges_into(
                        &mut graph,
                        new_idx,
                        &mut new_specs,
                        &mut seen_specs,
                        &mut edge_targets,
                        &cache,
                        &build_config,
                    );
                }
            }
        }

        stats.sum_graph_us += graph_start.elapsed().as_micros() as u64;
        stats.new_specs_emitted += new_specs.len();

        // Always reply (even if empty) so main loop's `in_flight`
        // counter decrements for each FetchEventMsg sent.
        if specs_tx.send(new_specs).await.is_err() {
            // Main loop dropped the receiver — bail.
            break;
        }
    }

    stats.unresolved_remaining = edge_targets.len();
    Ok((graph, stats))
}

/// Same as `enqueue_node_edges` but pushes new specs into the
/// caller-provided `out` Vec instead of a VecDeque. Used by the
/// graph worker to batch "new specs from this fetch" before sending
/// them back to the main loop in one channel message.
fn enqueue_node_edges_into(
    graph: &mut DependencyGraph,
    node_idx: NodeIndex,
    out: &mut Vec<Dep>,
    seen_specs: &mut HashSet<(String, String)>,
    edge_targets: &mut EdgeTargets,
    cache: &MemoryCache,
    build_config: &BuildDepsConfig,
) {
    let mut work_stack: Vec<NodeIndex> = vec![node_idx];
    while let Some(idx) = work_stack.pop() {
        let edges = collect_unresolved_edges(graph, idx);
        for edge in edges {
            if !edge.spec.is_registry_spec() {
                continue;
            }
            let key = (edge.name.clone(), edge.spec.clone());

            if let Some(core_arc) = cache.get_version_manifest(&edge.name, &edge.spec) {
                let resolved = ResolvedPackage {
                    name: edge.name.clone(),
                    version: core_arc.version.clone(),
                    manifest: core_arc,
                };
                let edge_info = crate::resolver::edges::DependencyEdgeInfo {
                    edge_id: edge.edge_id,
                    name: edge.name.clone(),
                    spec: edge.spec.clone(),
                    edge_type: edge.edge_type,
                };
                if let ProcessResult::Created(new_idx) = process_dependency_with_resolved(
                    graph,
                    idx,
                    &edge_info,
                    &resolved,
                    build_config,
                ) {
                    work_stack.push(new_idx);
                }
                continue;
            }

            edge_targets
                .entry(key.clone())
                .or_default()
                .push((idx, edge.edge_id));
            if seen_specs.insert(key.clone()) {
                out.push(key);
            }
        }
    }
}
