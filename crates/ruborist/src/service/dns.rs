//! DNS caching resolver (native only).
//!
//! Wraps the system resolver (`getaddrinfo`) with an in-memory cache to avoid
//! repeated DNS lookups for the same hostname during a single run.
//!
//! This is also more compatible with sandboxed environments (e.g. Claude Code)
//! where `hickory-dns` cannot reach DNS servers directly, but the OS resolver
//! (via `mDNSResponder` on macOS) still works.
//!
//! On WASM targets, DNS resolution is handled by the browser, so this module
//! is compiled out entirely.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, LazyLock};
    use std::time::{Duration, Instant};

    use parking_lot::RwLock;
    use reqwest::dns::{Addrs, Name, Resolve, Resolving};

    const DNS_CACHE_TTL: Duration = Duration::from_secs(300);

    static SHARED_RESOLVER: LazyLock<Arc<CachingResolver>> =
        LazyLock::new(|| Arc::new(CachingResolver::new(DNS_CACHE_TTL)));

    /// Return the global shared DNS resolver instance.
    pub fn shared_resolver() -> Arc<CachingResolver> {
        SHARED_RESOLVER.clone()
    }

    struct CacheEntry {
        addrs: Vec<SocketAddr>,
        expires_at: Instant,
    }

    /// A DNS resolver that caches results from the system resolver (`getaddrinfo`).
    ///
    /// Thread-safe and designed to be shared across all HTTP clients via `Arc`.
    pub struct CachingResolver {
        cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
        ttl: Duration,
    }

    impl CachingResolver {
        /// Create a new caching resolver with the given TTL for cache entries.
        pub(crate) fn new(ttl: Duration) -> Self {
            Self {
                cache: Arc::new(RwLock::new(HashMap::new())),
                ttl,
            }
        }
    }

    impl Resolve for CachingResolver {
        fn resolve(&self, name: Name) -> Resolving {
            let hostname = name.as_str().to_string();
            let cache = self.cache.clone();
            let ttl = self.ttl;

            // Fast path: check cache under read lock
            {
                let cache_read = cache.read();
                if let Some(entry) = cache_read.get(&hostname)
                    && entry.expires_at > Instant::now()
                {
                    let addrs: Addrs = Box::new(entry.addrs.clone().into_iter());
                    return Box::pin(std::future::ready(Ok(addrs)));
                }
            }

            // Cache miss or expired: resolve via system DNS and cache the result
            Box::pin(async move {
                let addrs: Vec<SocketAddr> = tokio::net::lookup_host((hostname.as_str(), 0))
                    .await
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                    .collect();

                if !addrs.is_empty() {
                    let mut cache_write = cache.write();
                    cache_write.insert(
                        hostname,
                        CacheEntry {
                            addrs: addrs.clone(),
                            expires_at: Instant::now() + ttl,
                        },
                    );
                }

                let addrs: Addrs = Box::new(addrs.into_iter());
                Ok(addrs)
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
