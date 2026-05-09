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
use serde::Deserialize;

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::PeerDeps;
use crate::resolver::preload::{Dep, PreloadConfig};
use crate::resolver::version::resolve_target_version;
use crate::service::MemoryCache;
use crate::spec::SpecStr;

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
    name: String,
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
        let url = format!("{}/{}", registry_url, name);
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
                    transitives: Vec::new(),
                    fetched: true,
                    wall_us,
                    net_us: wall_us,
                };
            }
        };
        let net_us = fut_start.elapsed().as_micros() as u64;
        let raw_arc: Arc<[u8]> = Arc::from(raw_bytes.as_ref());
        // Stash in body_cache early so concurrent sibling specs
        // arriving slightly after see it on their pending pop.
        body_cache.lock().insert(name.clone(), Arc::clone(&raw_arc));

        let spec_for_parse = spec.clone();
        let peer = peer_deps;
        let parsed =
            tokio::task::spawn_blocking(move || parse_combined(raw_arc, &spec_for_parse, peer))
                .await
                .ok()
                .flatten();

        let transitives = match parsed {
            Some((full_arc, resolved, core_arc, transitives)) => {
                cache.set_full_manifest(name.clone(), Arc::clone(&full_arc));
                cache.set_version_manifest(name.clone(), spec, Arc::clone(&core_arc));
                cache.set_version_manifest(name.clone(), resolved, core_arc);
                transitives
            }
            None => Vec::new(),
        };

        let wall_us = fut_start.elapsed().as_micros() as u64;
        FetchOutcome {
            name,
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
        let spec_for_parse = spec.clone();
        let peer = peer_deps;
        let parsed = tokio::task::spawn_blocking(move || {
            parse_combined(Arc::clone(&raw), &spec_for_parse, peer)
        })
        .await
        .ok()
        .flatten();

        let transitives = match parsed {
            Some((full_arc, resolved, core_arc, transitives)) => {
                // Don't overwrite full_manifest — the original fetcher
                // already set it. Only populate the version-manifest
                // slots so BFS hits the (name, spec) early-return.
                cache.set_full_manifest(name.clone(), full_arc);
                cache.set_version_manifest(name.clone(), spec, Arc::clone(&core_arc));
                cache.set_version_manifest(name.clone(), resolved, core_arc);
                transitives
            }
            None => Vec::new(),
        };

        let wall_us = fut_start.elapsed().as_micros() as u64;
        FetchOutcome {
            name,
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

    // Sibling-fetch dedup: when two specs for the same name are both
    // in flight, only the first fires a fetch; the second arrives at
    // the cached body and goes through `spawn_settle` instead.
    let body_cache: Arc<Mutex<HashMap<String, Arc<[u8]>>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut in_flight_names: HashSet<String> = HashSet::new();
    let mut deferred_by_name: HashMap<String, Vec<String>> = HashMap::new();

    let mut futs: FuturesUnordered<Fut> = FuturesUnordered::new();

    loop {
        // Refill to cap.
        while futs.len() < cap {
            let Some((name, spec)) = pending.pop_front() else {
                break;
            };
            // Sibling fast path: body already cached.
            if let Some(raw) = body_cache.lock().get(&name).cloned() {
                futs.push(spawn_settle(name, spec, raw, cache.clone(), peer_deps));
                continue;
            }
            // Defer if a fetch for this name is already in flight.
            if !in_flight_names.insert(name.clone()) {
                deferred_by_name.entry(name).or_default().push(spec);
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
            && let Some(siblings) = deferred_by_name.remove(&out.name)
            && let Some(raw) = body_cache.lock().get(&out.name).cloned()
        {
            for sibling_spec in siblings {
                futs.push(spawn_settle(
                    out.name.clone(),
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
