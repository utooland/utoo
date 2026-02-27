//! HTTP client infrastructure for registry operations.
//!
//! Provides the shared HTTP client with connection pooling, DNS caching (native),
//! and proxy support. Manifest fetching logic lives in `manifest.rs`.

use std::sync::LazyLock;

/// Global HTTP client with connection pooling and DNS caching.
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    client_builder()
        .build()
        .expect("Failed to build reqwest client")
});

pub(crate) fn get_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
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
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();

    #[cfg(not(target_arch = "wasm32"))]
    let builder = {
        use crate::service::dns::shared_resolver;

        let mut builder = builder
            .use_rustls_tls()
            .no_proxy()
            .dns_resolver(shared_resolver());

        match env_var("ALL_PROXY") {
            Some(url) => {
                builder = builder.proxy(reqwest::Proxy::all(&url).expect("invalid ALL_PROXY url"));
            }
            None => {
                // HTTPS_PROXY and HTTP_PROXY are checked independently (both can be set)
                if let Some(url) = env_var("HTTPS_PROXY") {
                    builder = builder
                        .proxy(reqwest::Proxy::https(&url).expect("invalid HTTPS_PROXY url"));
                }
                if let Some(url) = env_var("HTTP_PROXY") {
                    builder =
                        builder.proxy(reqwest::Proxy::http(&url).expect("invalid HTTP_PROXY url"));
                }
            }
        }

        builder
    };

    builder
}

#[cfg(not(target_arch = "wasm32"))]
fn env_var(key: &str) -> Option<String> {
    std::env::var(key)
        .or_else(|_| std::env::var(key.to_lowercase()))
        .ok()
}
