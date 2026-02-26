//! HTTP client infrastructure for registry operations.
//!
//! Provides the shared HTTP client with connection pooling, DNS caching,
//! and proxy support. Manifest fetching logic lives in `manifest.rs`.

use std::env;
use std::sync::LazyLock;

use crate::service::dns::shared_resolver;

/// Global HTTP client with connection pooling and DNS caching.
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    client_builder()
        .build()
        .expect("Failed to build reqwest client")
});

pub(crate) fn get_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}

/// Create a [`reqwest::ClientBuilder`] with rustls TLS, DNS caching, and proxy
/// from environment variables.
///
/// This is the shared base for all HTTP clients. Callers can further customize
/// the builder (e.g. add `user_agent`, `http1_only`, timeouts) before building.
///
/// Proxy is read once from environment variables:
/// `ALL_PROXY` > `HTTPS_PROXY` / `HTTP_PROXY` (and their lowercase variants).
pub fn client_builder() -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .dns_resolver(shared_resolver());

    if let Some(url) = env_var("ALL_PROXY") {
        builder = builder.proxy(reqwest::Proxy::all(&url).expect("invalid ALL_PROXY url"));
    } else {
        if let Some(url) = env_var("HTTPS_PROXY") {
            builder = builder.proxy(reqwest::Proxy::https(&url).expect("invalid HTTPS_PROXY url"));
        }
        if let Some(url) = env_var("HTTP_PROXY") {
            builder = builder.proxy(reqwest::Proxy::http(&url).expect("invalid HTTP_PROXY url"));
        }
    }

    builder
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).or_else(|_| env::var(key.to_lowercase())).ok()
}
