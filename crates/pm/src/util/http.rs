use std::sync::LazyLock;

use anyhow::{Context, Result, anyhow};

/// Global shared client for general-purpose HTTP requests.
///
/// Stores `Result` so proxy-configuration errors surface to callers on first
/// use instead of panicking. For download-heavy workloads with custom
/// timeouts, use [`client_builder`] instead.
static CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(|| {
    client_builder()
        .and_then(|b| b.build().context("Failed to build HTTP client"))
        .map_err(|e| e.to_string())
});

/// Return the global shared [`reqwest::Client`].
pub fn client() -> Result<&'static reqwest::Client> {
    CLIENT.as_ref().map_err(|e| anyhow!("{e}"))
}

/// pm's shared [`reqwest::ClientBuilder`]: ruborist's base policy (rustls
/// TLS, caching DNS resolver, connect timeout, HTTP/1.1 connection pool, and
/// env-var proxy handling with empty-string filtering) plus the utoo user
/// agent. See `utoo_ruborist::service::client_builder` for the policy details.
pub fn client_builder() -> Result<reqwest::ClientBuilder> {
    Ok(utoo_ruborist::service::client_builder()?
        .user_agent(concat!("utoo/", env!("CARGO_PKG_VERSION"))))
}
