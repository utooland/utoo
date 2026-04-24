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
use std::sync::atomic::{AtomicBool, Ordering};
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
    HTTP_CLIENT.as_ref().map_err(|e| anyhow!("{e}"))
}

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
    let builder = reqwest::Client::builder();

    #[cfg(not(target_arch = "wasm32"))]
    let builder = {
        use crate::service::dns::shared_resolver;

        let tls_config = build_rustls_config()?;
        let mut builder = builder
            .use_preconfigured_tls(tls_config)
            .no_proxy()
            .dns_resolver(shared_resolver())
            // Force HTTP/1.1 with a connection pool. reqwest multiplexes all
            // requests over a single HTTP/2 connection by default, which
            // makes head-of-line blocking on one slow response stall the
            // whole manifest fetch phase. An H1 pool lets concurrent
            // manifest requests open independent TCP streams instead.
            // Pool size matches `preload::DEFAULT_CONCURRENCY` so the
            // per-host idle pool can absorb every in-flight fetch without
            // churning connections.
            .http1_only()
            .pool_max_idle_per_host(256);

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
