//! Registry selection utilities.
//!
//! Provides functions for selecting the fastest npm registry by ping latency.

/// Default registries for auto-selection
pub const REGISTRY_NPMMIRROR: &str = "https://registry.npmmirror.com";
pub const REGISTRY_NPMJS: &str = "https://registry.npmjs.org";

/// Ping timeout (ms)
const PING_TIMEOUT_MS: u64 = 3000;

/// Ping result structure
struct PingResult {
    registry: String,
    latency_ms: u64,
    success: bool,
}

/// Ping a registry and measure latency
async fn ping_registry(registry_url: &str) -> PingResult {
    let ping_url = format!("{}/-/ping", registry_url);
    let start = std::time::Instant::now();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(PING_TIMEOUT_MS))
        .build();

    match client {
        Ok(client) => match client.get(&ping_url).send().await {
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
        },
        Err(_) => PingResult {
            registry: registry_url.to_string(),
            latency_ms: u64::MAX,
            success: false,
        },
    }
}

/// Select fastest registry by concurrent ping
pub async fn select_fastest_registry() -> String {
    let registries = [REGISTRY_NPMMIRROR, REGISTRY_NPMJS];
    let futures: Vec<_> = registries.iter().map(|r| ping_registry(r)).collect();
    let results = futures::future::join_all(futures).await;

    match results
        .into_iter()
        .filter(|r| r.success)
        .min_by_key(|r| r.latency_ms)
    {
        Some(r) => {
            tracing::info!(
                "Auto-selected registry: {} ({}ms)",
                r.registry,
                r.latency_ms
            );
            r.registry
        }
        None => {
            tracing::warn!(
                "All registry pings failed, using default: {}",
                REGISTRY_NPMMIRROR
            );
            REGISTRY_NPMMIRROR.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_constants() {
        assert_eq!(REGISTRY_NPMMIRROR, "https://registry.npmmirror.com");
        assert_eq!(REGISTRY_NPMJS, "https://registry.npmjs.org");
    }

    #[tokio::test]
    async fn test_ping_registry_invalid_url() {
        let result = ping_registry("http://invalid.registry.test").await;
        assert!(!result.success);
        assert_eq!(result.latency_ms, u64::MAX);
    }

    #[tokio::test]
    async fn test_ping_registry_timeout() {
        // Use a non-routable IP to trigger timeout
        let result = ping_registry("http://10.255.255.1").await;
        assert!(!result.success);
        assert_eq!(result.latency_ms, u64::MAX);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_ping_registry_npmmirror() {
        let result = ping_registry(REGISTRY_NPMMIRROR).await;
        assert!(result.success);
        assert!(result.latency_ms < PING_TIMEOUT_MS);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_ping_registry_npmjs() {
        let result = ping_registry(REGISTRY_NPMJS).await;
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
