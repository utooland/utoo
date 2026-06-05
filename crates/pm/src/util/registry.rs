//! Registry selection utilities.
//!
//! Provides functions for selecting the fastest npm registry by ping latency.

use colored::Colorize;

use super::http::client_builder;

/// Default registries for auto-selection
pub const REGISTRY_NPMMIRROR: &str = "https://registry.npmmirror.com";
pub const REGISTRY_NPMJS: &str = "https://registry.npmjs.org";

/// Ping timeout (ms)
const PING_TIMEOUT_MS: u64 = 3000;

/// Ping result structure
pub struct PingResult {
    pub registry: String,
    pub latency_ms: u64,
    pub success: bool,
}

/// Ping a registry and measure latency
pub async fn ping_registry(client: &reqwest::Client, registry_url: &str) -> PingResult {
    let ping_url = format!("{}/-/ping", registry_url);
    let start = std::time::Instant::now();

    match client.get(&ping_url).send().await {
        Ok(resp) if resp.status().is_success() => PingResult {
            registry: registry_url.to_string(),
            latency_ms: start.elapsed().as_millis() as u64,
            success: true,
        },
        _ => PingResult {
            registry: registry_url.to_string(),
            latency_ms: u64::MAX,
            success: false,
        },
    }
}

/// Select fastest registry by concurrent ping
pub async fn select_fastest_registry() -> String {
    let client = client_builder()
        .timeout(std::time::Duration::from_millis(PING_TIMEOUT_MS))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => {
            tracing::debug!("Failed to create HTTP client, using default registry");
            return REGISTRY_NPMMIRROR.to_string();
        }
    };

    let registries = [REGISTRY_NPMMIRROR, REGISTRY_NPMJS];
    let futures: Vec<_> = registries
        .iter()
        .map(|r| ping_registry(&client, r))
        .collect();
    let results = futures::future::join_all(futures).await;

    let (registry, latency_info) = match results
        .into_iter()
        .filter(|r| r.success)
        .min_by_key(|r| r.latency_ms)
    {
        Some(r) => {
            tracing::debug!("Auto-selected registry {} ({}ms)", r.registry, r.latency_ms);
            (
                r.registry,
                format!("{}ms", r.latency_ms).green().to_string(),
            )
        }
        None => {
            tracing::debug!("All registry pings failed, using default");
            (REGISTRY_NPMMIRROR.to_string(), "ping failed".to_string())
        }
    };

    eprintln!(
        "{} {} ({})",
        "Registry:".dimmed(),
        registry.cyan(),
        latency_info
    );
    eprintln!(
        "{} {}",
        "Tip:".dimmed(),
        format!("ut config set registry {} --global", registry).yellow()
    );

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_constants() {
        assert_eq!(REGISTRY_NPMMIRROR, "https://registry.npmmirror.com");
        assert_eq!(REGISTRY_NPMJS, "https://registry.npmjs.org");
    }

    fn create_test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(PING_TIMEOUT_MS))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_ping_registry_invalid_url() {
        let client = create_test_client();
        let result = ping_registry(&client, "http://invalid.registry.test").await;
        assert!(!result.success);
        assert_eq!(result.latency_ms, u64::MAX);
    }

    #[tokio::test]
    async fn test_ping_registry_timeout() {
        // Use a non-routable IP to trigger timeout
        let client = create_test_client();
        let result = ping_registry(&client, "http://10.255.255.1").await;
        assert!(!result.success);
        assert_eq!(result.latency_ms, u64::MAX);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_ping_registry_npmmirror() {
        let client = create_test_client();
        let result = ping_registry(&client, REGISTRY_NPMMIRROR).await;
        assert!(result.success);
        assert!(result.latency_ms < PING_TIMEOUT_MS);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_ping_registry_npmjs() {
        let client = create_test_client();
        let result = ping_registry(&client, REGISTRY_NPMJS).await;
        assert!(result.success);
        assert!(result.latency_ms < PING_TIMEOUT_MS);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_select_fastest_registry() {
        let registry = select_fastest_registry().await;
        assert!(registry == REGISTRY_NPMMIRROR || registry == REGISTRY_NPMJS);
    }
}
