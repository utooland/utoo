use std::env;
use std::sync::LazyLock;

/// Global shared client for general-purpose HTTP requests.
///
/// Uses the same no-system-proxy policy as [`client_builder`].
/// For download-heavy workloads with custom timeouts, use [`client_builder`] instead.
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    client_builder()
        .build()
        .expect("Failed to build HTTP client")
});

/// Return a reference to the global shared [`reqwest::Client`].
pub fn client() -> &'static reqwest::Client {
    &CLIENT
}

/// Create a [`reqwest::ClientBuilder`] with system proxy auto-detection disabled.
///
/// reqwest's `system-proxy` feature (enabled via Cargo feature unification)
/// reads macOS / Windows system proxy settings (e.g. set by Surge / Clash),
/// which can cause unexpected routing or significant slowdowns.
///
/// This builder only respects explicitly set environment variables:
/// `ALL_PROXY`, `HTTPS_PROXY`, `HTTP_PROXY` (and their lowercase variants).
pub fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("utoo/", env!("CARGO_PKG_VERSION")))
        .no_proxy()
        .proxy(reqwest::Proxy::custom(|url| {
            env_var("ALL_PROXY").or_else(|| match url.scheme() {
                "https" => env_var("HTTPS_PROXY"),
                _ => env_var("HTTP_PROXY"),
            })
        }))
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).or_else(|_| env::var(key.to_lowercase())).ok()
}
