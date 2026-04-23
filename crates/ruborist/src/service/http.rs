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
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow};

/// Number of parallel reqwest clients fronting the registry.
///
/// Each client keeps its own connection pool. The real benefit of splitting
/// across multiple clients isn't extra parallelism (the resolver is still
/// 64-concurrent) — it's breaking the phase-lock where 64 synchronous
/// responses leave 64 connections idle in lockstep. Two clients let one
/// pool be mid-transfer while the other is in the handoff gap.
///
/// 4 clients × 64 pre-warm handshakes was measurably slower in CI runs
/// because the 256 concurrent TLS handshakes ate too much of the 4-core
/// runner before resolve work started. 2 keeps the stagger benefit and
/// halves the preheat startup cost.
const HTTP_CLIENT_COUNT: usize = 2;

/// Global pool of reqwest clients with connection pooling and DNS caching.
///
/// Stores `Result<Vec<Client>, String>` so proxy-configuration errors are
/// surfaced to callers instead of panicking or calling `process::exit`.
static HTTP_CLIENTS: LazyLock<Result<Vec<reqwest::Client>, String>> = LazyLock::new(|| {
    (0..HTTP_CLIENT_COUNT)
        .map(|_| client_builder().and_then(|b| b.build().context("Failed to build reqwest client")))
        .collect::<Result<Vec<_>>>()
        .map_err(|e| e.to_string())
});

/// Round-robin cursor for `pick_client()`.
static CLIENT_RR: AtomicUsize = AtomicUsize::new(0);

/// Hand out one of the global clients, cycling through them in round-robin
/// order. Each call advances the cursor by 1 so consecutive manifest
/// fetches land on different pools, spreading TCP connections across all
/// `HTTP_CLIENT_COUNT` clients.
pub(crate) fn pick_client() -> Result<&'static reqwest::Client> {
    let clients = HTTP_CLIENTS.as_ref().map_err(|e| anyhow!("{e}"))?;
    let idx = CLIENT_RR.fetch_add(1, Ordering::Relaxed) % clients.len();
    Ok(&clients[idx])
}

/// Warm every client's connection pool by firing `per_client` parallel HEAD
/// requests at `url` from **each** client concurrently.
///
/// Why multi-client matters: a single reqwest Client dedupes via its pool,
/// so 256 concurrent HEADs on one client all race into the same pool — the
/// first ~64 open TCPs, the rest reuse as soon as the first responses land.
/// 4 isolated clients × 64 HEADs each force 4 × 64 = 256 truly parallel
/// TCP connects, because the pools don't share state.
///
/// Errors are intentionally swallowed: a failed HEAD simply leaves that
/// slot "cold" — functionally equivalent to no preheat for that slot.
pub async fn preheat(url: &str, per_client: usize) {
    use futures::future::join_all;

    let Ok(clients) = HTTP_CLIENTS.as_ref() else {
        return;
    };

    let tasks = clients
        .iter()
        .flat_map(|client| (0..per_client).map(move |_| client.head(url).send()))
        .map(|fut| async move {
            let _ = fut.await;
        });
    join_all(tasks).await;
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

        let mut builder = builder
            .use_rustls_tls()
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
