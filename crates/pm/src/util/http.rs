use std::env;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use utoo_ruborist::service::dns::CachingResolver;

/// Global DNS caching resolver shared across all HTTP clients.
static DNS_RESOLVER: LazyLock<Arc<CachingResolver>> =
    LazyLock::new(|| Arc::new(CachingResolver::new(Duration::from_secs(300))));

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

/// Create a [`reqwest::ClientBuilder`] with system proxy auto-detection disabled
/// and DNS caching enabled.
///
/// Proxy is determined once at build time from environment variables
/// (`ALL_PROXY` > `HTTPS_PROXY` / `HTTP_PROXY`), then configured as a static
/// proxy so that connections are properly pooled and file descriptors reused.
///
/// DNS resolution uses the system resolver (`getaddrinfo`) with a 5-minute
/// in-memory cache, replacing `hickory-dns` which may fail in sandboxed
/// environments.
pub fn client_builder() -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("utoo/", env!("CARGO_PKG_VERSION")))
        .no_proxy()
        .dns_resolver(DNS_RESOLVER.clone());

    builder = match env_proxy() {
        Some(EnvProxy::All(url)) => builder.proxy(reqwest::Proxy::all(&url).expect("invalid ALL_PROXY url")),
        Some(EnvProxy::PerScheme { https, http }) => {
            if let Some(url) = https {
                builder = builder.proxy(reqwest::Proxy::https(&url).expect("invalid HTTPS_PROXY url"));
            }
            if let Some(url) = http {
                builder = builder.proxy(reqwest::Proxy::http(&url).expect("invalid HTTP_PROXY url"));
            }
            builder
        }
        None => builder,
    };

    builder
}

enum EnvProxy {
    All(String),
    PerScheme {
        https: Option<String>,
        http: Option<String>,
    },
}

/// Read proxy configuration from environment variables once.
fn env_proxy() -> Option<EnvProxy> {
    if let Some(url) = env_var("ALL_PROXY") {
        return Some(EnvProxy::All(url));
    }
    let https = env_var("HTTPS_PROXY");
    let http = env_var("HTTP_PROXY");
    if https.is_some() || http.is_some() {
        return Some(EnvProxy::PerScheme { https, http });
    }
    None
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).or_else(|_| env::var(key.to_lowercase())).ok()
}
