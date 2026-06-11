use std::fmt;
use tokio::time::Duration;
use tokio_retry::strategy::ExponentialBackoff;

use super::http::client_builder;

/// A generic error type for retryable operations
#[derive(Debug)]
pub enum RetryableError {
    Permanent(String), // Non-retryable error
    Temporary(String), // Retryable error
}

impl fmt::Display for RetryableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RetryableError::Permanent(e) => write!(f, "{e}"),
            RetryableError::Temporary(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RetryableError {}

/// Build a reqwest::Client with timeout config.
/// Connection pool is unlimited - concurrency is controlled by semaphore instead.
pub fn build_dns_cached_client() -> reqwest::Client {
    client_builder()
        .connect_timeout(std::time::Duration::from_secs(5)) // TLS + TCP handshake
        .read_timeout(std::time::Duration::from_secs(30)) // Timeout for individual read operations
        // HTTP/1.1 so each in-flight tarball gets its own TCP connection.
        // With ALPN h2 the registry CDN multiplexes every stream over one
        // connection (single congestion window + flow-control cap, loss on
        // one stream stalls siblings) — same rationale as the manifest
        // client in ruborist/src/service/http.rs.
        .http1_only()
        // Tarballs are already gzip; don't invite a second Content-Encoding
        // layer that would just burn CPU on decode.
        .no_gzip()
        // No total timeout - large files (e.g. node binary ~100MB) need longer download time
        // No pool_max_idle_per_host - let reqwest manage connections freely
        // Concurrency is bounded by the resolver's in-flight fetch cap
        .build()
        .expect("Failed to build reqwest client")
}

pub fn create_retry_strategy() -> impl Iterator<Item = Duration> {
    let delays = vec![
        Duration::from_millis(100), // 100ms
        Duration::from_millis(200), // 200ms
        Duration::from_secs(1),     // 1s
        Duration::from_secs(1),     // 1s
        Duration::from_secs(1),     // 1s
    ];
    let exp_strategy = ExponentialBackoff::from_millis(1000)
        .max_delay(Duration::from_secs(20))
        .take(5); // 5 fixed delays

    delays.into_iter().chain(exp_strategy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_delays() {
        let strategy = create_retry_strategy();
        let delays: Vec<Duration> = strategy.take(5).collect();

        assert_eq!(delays[0], Duration::from_millis(100));
        assert_eq!(delays[1], Duration::from_millis(200));
        assert_eq!(delays[2], Duration::from_secs(1));
        assert_eq!(delays[3], Duration::from_secs(1));
        assert_eq!(delays[4], Duration::from_secs(1));
    }

    #[test]
    fn test_total_retry_count() {
        let strategy = create_retry_strategy();
        let delays: Vec<Duration> = strategy.collect();

        assert_eq!(delays.len(), 10);
    }

    #[test]
    fn test_max_delay_limit() {
        let strategy = create_retry_strategy();
        let max_delay = Duration::from_secs(20);

        for delay in strategy {
            assert!(delay <= max_delay, "Delay should not exceed 20 seconds");
        }
    }
}
