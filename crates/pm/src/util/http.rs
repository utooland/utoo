use std::sync::LazyLock;
use std::time::Duration;

/// Global shared client for pm-specific HTTP requests (auth, binary downloads).
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    client_builder()
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client")
});

/// Return a reference to the global shared [`reqwest::Client`].
pub fn client() -> &'static reqwest::Client {
    &CLIENT
}

/// Create a [`reqwest::ClientBuilder`] with user-agent, rustls TLS, DNS caching,
/// and proxy from environment variables.
///
/// Builds on top of [`utoo_ruborist::service::client_builder`] which provides
/// the shared base configuration (rustls, DNS cache, proxy).
pub fn client_builder() -> reqwest::ClientBuilder {
    utoo_ruborist::service::client_builder().user_agent(concat!("utoo/", env!("CARGO_PKG_VERSION")))
}
