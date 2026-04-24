//! HTTP request chain for registry operations.
//!
//! # Architecture
//!
//! ```text
//!   registry.rs                      resolve_package(name, spec)
//!       |                                     |
//!       |          +------------- supports_semver? -------------+
//!       |          |                                            |
//!       |        true (npmmirror)                     false (npmjs.org)
//!       |          |                                            |
//!       |   fetch_version_manifest          resolve_full_manifest
//!       |   GET /{name}/{spec}              GET /{name}
//!       |   Accept: abbreviated             Accept: abbreviated
//!       |          |                        + If-None-Match: {etag}
//!       |          |                                  |
//!       |          |                        +---------+---------+
//!       |          |                      200 OK          304 Not Modified
//!       |          |                    parse manifest     use disk cache
//!       |          |                    cache etag         + fetch version
//!       |          |                        |                   |
//!       v          v                        v                   v
//!  +-----------------------------------------------------------------+
//!  |  manifest.rs -- Retry Layer                                     |
//!  |  RetryIf + FetchError { Retryable, Permanent }                  |
//!  |  delays: 100ms -> 200ms -> 500ms -> 1s -> 2s  (5 attempts max) |
//!  +-----------------------------------------------------------------+
//!       |
//!       v
//!  +-----------------------------------------------------------------+
//!  |  http.rs -- HTTP Client  (this file)                            |
//!  |  global singleton reqwest::Client (LazyLock)                    |
//!  |  rustls TLS + no_proxy + env proxy + CachingResolver            |
//!  +-----------------------------------------------------------------+
//!       |
//!       v
//!  +-----------------------------------------------------------------+
//!  |  dns.rs -- CachingResolver                                      |
//!  |  wraps OS getaddrinfo + in-memory cache (TTL 300s)              |
//!  |  single-flight: concurrent lookups coalesced via OnceCell       |
//!  +-----------------------------------------------------------------+
//! ```
//!
//! # Abbreviated metadata
//!
//! `Accept: application/vnd.npm.install-v1+json` returns only install-relevant
//! fields (deps, dist, engines, bin), 10-50x smaller than full JSON.
//! Controlled by [`manifest::MetadataFormat`].
//!
//! # ETag / 304 Not Modified
//!
//! Only for `fetch_full_manifest` on non-semver registries. First request
//! returns `ETag`; subsequent requests send `If-None-Match`. On 304, the
//! disk-cached version list is reused and individual versions are fetched
//! separately. See [`manifest::FetchManifestResult`].
//!
//! # Retry classification
//!
//! Errors are classified structurally (not by string matching):
//! - **Retryable**: timeout, connect, body (stream reset), request (h2 reset),
//!   HTTP 429, HTTP 5xx
//! - **Permanent**: HTTP 404, JSON parse, other 4xx
//!
//! See [`manifest::FetchError`] and [`manifest::classify_status`].
//!
//! # Proxy
//!
//! System proxy is disabled (`no_proxy`). Only env vars are read:
//! `ALL_PROXY` > `HTTPS_PROXY` + `HTTP_PROXY` (+ lowercase variants).
//!
//! # DNS caching
//!
//! Replaces `hickory-dns` with OS resolver (`getaddrinfo` via
//! `tokio::net::lookup_host`) wrapped in [`dns::CachingResolver`].
//! More compatible with sandboxed environments where direct DNS is blocked.
//! WASM targets skip DNS entirely (browser handles it).

use std::sync::LazyLock;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use crossbeam_queue::SegQueue;

/// Diagnostic: per-HTTP-request `(send_start, body_end)` timestamps.
///
/// When the flag is active, manifest fetch sites push `(Instant, Instant)`
/// pairs into the queue. Preload uses the collected intervals to compute
/// "pure network window", "busy window" (interval union), and per-request
/// stats — isolating network wait from our own CPU work.
///
/// Flag is a single `AtomicBool` (relaxed) and only manifests as one
/// comparison + two `Instant::now()` per HTTP request when enabled; zero
/// cost on the disabled path.
static HTTP_TRACE_ACTIVE: AtomicBool = AtomicBool::new(false);
static HTTP_TRACE: LazyLock<SegQueue<(Instant, Instant)>> = LazyLock::new(SegQueue::new);

/// Activate per-request HTTP timing capture. Drains any prior trace.
pub fn start_http_trace() {
    while HTTP_TRACE.pop().is_some() {}
    HTTP_TRACE_ACTIVE.store(true, Ordering::Relaxed);
}

/// Stop capturing and return the collected `(start, end)` intervals.
pub fn finish_http_trace() -> Vec<(Instant, Instant)> {
    HTTP_TRACE_ACTIVE.store(false, Ordering::Relaxed);
    let mut out = Vec::new();
    while let Some(v) = HTTP_TRACE.pop() {
        out.push(v);
    }
    out
}

/// Record one completed HTTP request's `(send_start, body_end)` timestamps.
/// No-op when the trace flag is off. Cheap relaxed-load check guards the
/// push so disabled callers pay almost nothing.
#[inline]
pub fn record_http_interval(start: Instant, end: Instant) {
    if HTTP_TRACE_ACTIVE.load(Ordering::Relaxed) {
        HTTP_TRACE.push((start, end));
    }
}

/// Diagnostic: per-parse `(queued_at, exec_start, exec_end)` timestamps.
///
/// `queued_at` is captured right before `spawn_blocking` is called;
/// `exec_start` is captured inside the closure (i.e. once the blocking
/// pool actually picks the task up). `queue_wait = exec_start − queued_at`
/// measures how long each parse sat idle in the blocking-pool queue.
/// When `queue_wait p50 ≫ exec p50` the pool is the bottleneck, and
/// `resolve_package` awaits stall the `FuturesUnordered` pipeline.
static PARSE_TRACE_ACTIVE: AtomicBool = AtomicBool::new(false);
static PARSE_TRACE: LazyLock<SegQueue<(Instant, Instant, Instant)>> = LazyLock::new(SegQueue::new);

pub fn start_parse_trace() {
    while PARSE_TRACE.pop().is_some() {}
    PARSE_TRACE_ACTIVE.store(true, Ordering::Relaxed);
}

pub fn finish_parse_trace() -> Vec<(Instant, Instant, Instant)> {
    PARSE_TRACE_ACTIVE.store(false, Ordering::Relaxed);
    let mut out = Vec::new();
    while let Some(v) = PARSE_TRACE.pop() {
        out.push(v);
    }
    out
}

#[inline]
pub fn parse_trace_enabled() -> bool {
    PARSE_TRACE_ACTIVE.load(Ordering::Relaxed)
}

#[inline]
pub fn record_parse_interval(queued_at: Instant, exec_start: Instant, exec_end: Instant) {
    if PARSE_TRACE_ACTIVE.load(Ordering::Relaxed) {
        PARSE_TRACE.push((queued_at, exec_start, exec_end));
    }
}

/// Global HTTP client with connection pooling and DNS caching.
///
/// Stores `Result<Client, String>` so that proxy-configuration errors are
/// surfaced to callers instead of panicking or calling `process::exit`.
///
/// Multi-client experiments (2 and 4 isolated clients + preheat) both
/// regressed p1_resolve on CI — preheat TLS handshake cost exceeded the
/// phase-lock smoothing benefit, and uv (which uses a single client)
/// proves one pool is the right default.
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    client_builder()
        .and_then(|b| b.build().context("Failed to build reqwest client"))
        .map_err(|e| e.to_string())
});

pub(crate) fn pick_client() -> Result<&'static reqwest::Client> {
    // Multi-client pool: round-robin across N clients, each pinned to a
    // distinct resolved IP of the registry host via `resolve_to_addrs`.
    // Forces per-IP connection distribution, replicating bun's 4×64 pcap
    // pattern instead of relying on Happy Eyeballs alone.
    if let Some(Ok(clients)) = CLIENT_POOL.get()
        && !clients.is_empty()
    {
        let idx = CLIENT_RR.fetch_add(1, Ordering::Relaxed) % clients.len();
        return Ok(&clients[idx]);
    }
    HTTP_CLIENT.as_ref().map_err(|e| anyhow!("{e}"))
}

/// Per-IP client pool. Populated lazily by [`init_client_pool`] at
/// [`UnifiedRegistry::build`] time. Result wraps errors so the init
/// failure doesn't panic — callers fall back to the shared-resolver
/// single client.
static CLIENT_POOL: OnceLock<Result<Vec<reqwest::Client>, String>> = OnceLock::new();
static CLIENT_RR: AtomicUsize = AtomicUsize::new(0);

/// Build one client per resolved registry IP (up to `CLIENT_POOL_MAX`).
///
/// npmjs.org anycasts to ~4 Cloudflare edges. Single-client + DNS
/// rotation lets Happy Eyeballs pick among them, but in practice CI's
/// reachable-IPv6 ordering biases >50 % of connections onto one IP
/// (observed in pcap). Explicit `resolve_to_addrs` per client pins each
/// client to one IP, guaranteeing that round-robin dispatch
/// distributes connections evenly: N=4 clients × 32 conns/client at
/// cap=128 matches bun's observed 4×64 at cap=256, scaled for our
/// lower cap.
///
/// Sync DNS via `ToSocketAddrs::to_socket_addrs` — blocks the calling
/// thread for one getaddrinfo lookup (~10-50 ms once per process). OK
/// because it's called from [`UnifiedRegistry::build`] which is sync
/// and runs once at startup.
#[cfg(not(target_arch = "wasm32"))]
pub fn init_client_pool(registry_url: &str) {
    use std::net::{SocketAddr, ToSocketAddrs};

    const CLIENT_POOL_MAX: usize = 4;

    let init = || -> Result<Vec<reqwest::Client>, String> {
        let url =
            reqwest::Url::parse(registry_url).map_err(|e| format!("parse registry url: {e}"))?;
        let host = url
            .host_str()
            .ok_or_else(|| "registry URL has no host".to_string())?
            .to_string();
        let port = url.port_or_known_default().unwrap_or(443);

        let addrs: Vec<SocketAddr> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("DNS lookup {host}:{port}: {e}"))?
            .collect();

        // Prefer IPv4 — each client is pinned to a single IP without
        // Happy Eyeballs fallback, so we must pick a family that's
        // universally reachable. GitHub Actions ubuntu runners have
        // working v4 to Cloudflare edges but v6 routing is blocked
        // (first attempt returned `os error 101 Network is unreachable`
        // on every pinned v6 client, wiping out the whole preload).
        // v4 works on CI, local dev, and everywhere we've tested.
        let v4: Vec<SocketAddr> = addrs.iter().filter(|a| a.is_ipv4()).copied().collect();
        let v6: Vec<SocketAddr> = addrs.iter().filter(|a| a.is_ipv6()).copied().collect();
        let selected: Vec<SocketAddr> = v4.into_iter().chain(v6).take(CLIENT_POOL_MAX).collect();

        if selected.is_empty() {
            return Err(format!("no addresses resolved for {host}"));
        }

        tracing::info!(
            "HTTP client pool: {} clients for {} ({} addrs resolved, {} selected)",
            selected.len(),
            host,
            addrs.len(),
            selected.len()
        );

        selected
            .into_iter()
            .map(|addr| {
                // Per-IP pinned clients negotiate HTTP/2 when the server
                // supports it (npmjs advertises h2 via ALPN). 4 separate
                // H2 connections avoid the single-H2 HoL trap from an
                // earlier revert: a slow response only stalls streams on
                // its own conn, not the whole phase. H2 stream
                // multiplexing also scales concurrency without bumping
                // TCP conn count, bypassing whatever per-TCP-conn rate
                // policing npmjs applies.
                client_builder_ext(HttpVersion::Negotiate)
                    .and_then(|b| {
                        b.resolve_to_addrs(&host, &[addr])
                            .build()
                            .context("build pinned reqwest client")
                    })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()
    };

    let _ = CLIENT_POOL.set(init());
}

#[cfg(target_arch = "wasm32")]
pub fn init_client_pool(_registry_url: &str) {}

/// Build a `rustls::ClientConfig` using the `aws-lc-rs` crypto provider
/// instead of reqwest's default `ring`. Measured on CI (4-core runner)
/// against npmjs.org, ring's per-TLS-handshake client-side CPU cost
/// (ECDHE key derivation + cert verification + Finished MAC) serialised
/// across 128 parallel handshakes into a 154 ms "CCS → first AppData"
/// span — the HTTP requests couldn't fire until all TLS crypto drained
/// through 4 async workers. aws-lc-rs uses BoringSSL's assembly-optimised
/// primitives and is roughly 3× faster at handshake work.
#[cfg(not(target_arch = "wasm32"))]
fn build_rustls_config() -> Result<rustls::ClientConfig> {
    // Install aws-lc-rs as the default for any other rustls consumer in
    // the process. Idempotent — only the first call per process wins.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Load OS root certs. On Linux this hits /etc/ssl/certs/,
    // on macOS it queries the Security framework keychain. Called once
    // per process via `HTTP_CLIENT`'s `LazyLock`.
    let roots = rustls_native_certs::load_native_certs();
    let mut root_store = rustls::RootCertStore::empty();
    for cert in roots.certs {
        // Best-effort: skip any cert rustls refuses (same tolerance
        // native-tls shows). A hard fail here would brick every
        // request on a box with one bad root in its trust store.
        let _ = root_store.add(cert);
    }
    if !roots.errors.is_empty() {
        tracing::debug!(
            "rustls-native-certs reported {} non-fatal load issues",
            roots.errors.len()
        );
    }

    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| anyhow!("rustls safe_default_protocol_versions: {e}"))?
    .with_root_certificates(root_store)
    .with_no_client_auth();

    Ok(config)
}

/// Create a [`reqwest::ClientBuilder`] with TLS, DNS caching, and proxy
/// from environment variables.
///
/// This is the shared base for all HTTP clients. Callers can further customize
/// the builder (e.g. add `user_agent`, `http1_only`, timeouts) before building.
///
/// On native targets: uses rustls TLS, caching DNS resolver, and reads proxy
/// from `ALL_PROXY` > `HTTPS_PROXY` / `HTTP_PROXY` (and their lowercase variants).
///
/// On WASM targets: returns a minimal builder (browser handles TLS, DNS, proxy).
///
/// Returns `Err` if a proxy URL from the environment is malformed.
pub fn client_builder() -> Result<reqwest::ClientBuilder> {
    client_builder_ext(HttpVersion::Http1Only)
}

/// HTTP protocol negotiation mode for [`client_builder_ext`].
#[derive(Copy, Clone, Debug)]
pub(crate) enum HttpVersion {
    /// Force HTTP/1.1 — a new TCP connection per in-flight request.
    /// Pcap comparison showed single-H2 reqwest (default) serialises
    /// all manifest requests through one multiplexed conn and hits
    /// HoL blocking on slow responses.
    Http1Only,
    /// Negotiate via ALPN — prefer HTTP/2 if server supports, fall
    /// back to HTTP/1.1. Intended for multi-client pool where each
    /// pinned client gets its own H2 conn, so HoL scope is
    /// 1/N-of-the-phase instead of whole-phase.
    Negotiate,
}

pub(crate) fn client_builder_ext(version: HttpVersion) -> Result<reqwest::ClientBuilder> {
    let builder = reqwest::Client::builder();

    #[cfg(not(target_arch = "wasm32"))]
    let builder = {
        use crate::service::dns::shared_resolver;

        let tls_config = build_rustls_config()?;
        let mut builder = builder
            .use_preconfigured_tls(tls_config)
            .no_proxy()
            .dns_resolver(shared_resolver())
            .pool_max_idle_per_host(256);
        builder = match version {
            HttpVersion::Http1Only => builder.http1_only(),
            HttpVersion::Negotiate => builder,
        };

        match env_var("ALL_PROXY") {
            Some(url) => {
                builder = builder.proxy(
                    reqwest::Proxy::all(&url)
                        .with_context(|| format!("invalid ALL_PROXY url: {url}"))?,
                );
            }
            None => {
                // HTTPS_PROXY and HTTP_PROXY are checked independently (both can be set)
                if let Some(url) = env_var("HTTPS_PROXY") {
                    builder = builder.proxy(
                        reqwest::Proxy::https(&url)
                            .with_context(|| format!("invalid HTTPS_PROXY url: {url}"))?,
                    );
                }
                if let Some(url) = env_var("HTTP_PROXY") {
                    builder = builder.proxy(
                        reqwest::Proxy::http(&url)
                            .with_context(|| format!("invalid HTTP_PROXY url: {url}"))?,
                    );
                }
            }
        }

        builder
    };

    Ok(builder)
}

/// Read a proxy env var, checking uppercase then lowercase.
///
/// Empty strings are treated as unset. This matters in Claude Code's sandbox,
/// which sets `ALL_PROXY=""` (empty uppercase) alongside `all_proxy=socks5://...`
/// (valid lowercase). Without filtering empties first, `std::env::var("ALL_PROXY")`
/// returns `Ok("")` — an `Ok`, not `Err` — so a naive `Result::or_else` fallback
/// to the lowercase variant would never fire.
#[cfg(not(target_arch = "wasm32"))]
fn env_var(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var(key.to_lowercase())
                .ok()
                .filter(|s| !s.is_empty())
        })
}
