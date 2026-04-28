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
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

/// Global HTTP client with connection pooling and DNS caching.
///
/// Stores `Result<Client, String>` so that proxy-configuration errors are
/// surfaced to callers instead of panicking or calling `process::exit`.
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    client_builder()
        .and_then(|b| b.build().context("Failed to build reqwest client"))
        .map_err(|e| e.to_string())
});

pub(crate) fn get_client() -> Result<&'static reqwest::Client> {
    HTTP_CLIENT.as_ref().map_err(|e| anyhow!("{e}"))
}

/// Build a `rustls::ClientConfig` using the `aws-lc-rs` crypto provider
/// instead of reqwest's default `ring`. Gated to macOS only — local
/// 8-run interleaved benchmark on M-series ARM (ant-design / npmmirror,
/// release-local profile) measured median 6.60 s → 3.85 s (-2.75 s,
/// 42 % faster) swapping the provider; CI bench-phases on Linux x86_64
/// + Mac CI showed +0.3-0.5 s regressions, so non-macOS keeps ring via
/// reqwest's default `rustls-tls-native-roots` feature.
#[cfg(target_os = "macos")]
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

        let mut builder = builder.no_proxy().dns_resolver(shared_resolver());

        // macOS: override TLS to aws-lc-rs (see `build_rustls_config`).
        // Linux/Windows: reqwest's default rustls + ring (via the
        // `rustls-tls-native-roots` feature in Cargo.toml).
        #[cfg(target_os = "macos")]
        {
            builder = builder.use_preconfigured_tls(build_rustls_config()?);
        }

        builder = builder
            // Without this, a hung TCP/TLS handshake holds its conn-slot
            // indefinitely. The retry layer in `service::fetch` only fires
            // on a reqwest error — if the socket is silently waiting at
            // SYN-ACK or ClientHello, no error surfaces and the request
            // waits forever. Observed on a ~400 ms-RTT wifi against
            // npmmirror: wall 22.6 s → 19.5 s, avg_conc 83 → 88, no
            // retry inflation, no false-positive cancellations.
            //
            // 5 s window leaves ~4× headroom over the worst RTTs we
            // expect (≤1.2 s for 400 ms RTT × 3 round-trips for full
            // TCP+TLS).
            //
            // No `.read_timeout()` or `.timeout()` is set: a per-read or
            // total-time cap risks killing legitimate slow body
            // downloads (cold-cache CDN edges that pause for >10 s mid-
            // body) and triggering a retry storm. CI bench-phases
            // showed an experimental `read_timeout(10s)` regressed
            // Linux ubuntu-latest npmmirror p1_resolve by +7 s with
            // σ=6.33 — cure worse than disease.
            .connect_timeout(Duration::from_secs(5));

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
