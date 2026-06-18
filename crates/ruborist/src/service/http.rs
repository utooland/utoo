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
//!       |   fetch version job              fetch full manifest job
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
//!  |  global reqwest::Client pool (LazyLock)                         |
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
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

#[cfg(not(target_arch = "wasm32"))]
use crate::service::dns::shared_resolver;

/// Cuts hung TCP/TLS handshakes that would otherwise pin a conn-slot
/// indefinitely — `service::fetch`'s retry layer only fires on a reqwest
/// error, which silently-stalled sockets never raise.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Number of independent registry HTTP client pools.
///
/// GHA npmjs pcap showed bun spreading resolve traffic across a few
/// Cloudflare edge IPs while a single reqwest pool concentrated requests on
/// one IP. Four pools keeps the model small but gives the resolver enough
/// independent keep-alive pools to fan out when npmjs/full-manifest
/// concurrency is raised.
#[cfg(not(target_arch = "wasm32"))]
const CLIENT_POOL_SIZE: usize = 4;

/// Global HTTP clients with connection pooling and DNS caching.
///
/// Stores `Result<Vec<Client>, String>` so that proxy-configuration errors are
/// surfaced to callers instead of panicking or calling `process::exit`.
#[cfg(not(target_arch = "wasm32"))]
static HTTP_CLIENTS: LazyLock<Result<Vec<reqwest::Client>, String>> = LazyLock::new(|| {
    (0..CLIENT_POOL_SIZE)
        .map(|_| client_builder().and_then(|b| b.build().context("Failed to build reqwest client")))
        .collect::<Result<Vec<_>>>()
        .map_err(|e| e.to_string())
});

#[cfg(not(target_arch = "wasm32"))]
static CLIENT_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn get_client() -> Result<&'static reqwest::Client> {
    let clients = HTTP_CLIENTS.as_ref().map_err(|e| anyhow!("{e}"))?;
    let idx = CLIENT_COUNTER.fetch_add(1, Ordering::Relaxed) % clients.len();
    Ok(&clients[idx])
}

/// WASM targets retain a single browser-backed client; there is no native TCP
/// connection pool to fan out.
#[cfg(target_arch = "wasm32")]
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    client_builder()
        .and_then(|b| b.build().context("Failed to build reqwest client"))
        .map_err(|e| e.to_string())
});

#[cfg(target_arch = "wasm32")]
pub(crate) fn get_client() -> Result<&'static reqwest::Client> {
    HTTP_CLIENT.as_ref().map_err(|e| anyhow!("{e}"))
}

/// Create a [`reqwest::ClientBuilder`] with TLS, DNS caching, and proxy
/// from environment variables.
///
/// This is the shared base for all HTTP clients. Callers can further customize
/// the builder (e.g. add `user_agent`, `http1_only`, timeouts) before building.
///
/// On native targets: uses reqwest's rustls TLS, caching DNS resolver, and
/// reads proxy from `ALL_PROXY` > `HTTPS_PROXY` / `HTTP_PROXY` (and their
/// lowercase variants).
///
/// On WASM targets: returns a minimal builder (browser handles TLS, DNS, proxy).
///
/// Returns `Err` if a proxy URL from the environment is malformed.
pub fn client_builder() -> Result<reqwest::ClientBuilder> {
    let builder = reqwest::Client::builder();

    #[cfg(not(target_arch = "wasm32"))]
    let builder = {
        let mut builder = builder
            .no_proxy()
            .dns_resolver(shared_resolver())
            .connect_timeout(CONNECT_TIMEOUT)
            // Force HTTP/1.1 with a connection pool. reqwest multiplexes all
            // requests over a single HTTP/2 connection by default, which
            // makes head-of-line blocking on one slow response stall the
            // whole manifest fetch phase. An H1 pool lets concurrent
            // manifest requests open independent TCP streams instead.
            .http1_only();

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
