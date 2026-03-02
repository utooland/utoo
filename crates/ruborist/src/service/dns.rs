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

    use parking_lot::Mutex;
    use reqwest::dns::{Addrs, Name, Resolve, Resolving};
    use tokio::sync::OnceCell;

    const DNS_CACHE_TTL: Duration = Duration::from_secs(300);

    static SHARED_RESOLVER: LazyLock<Arc<CachingResolver>> =
        LazyLock::new(|| Arc::new(CachingResolver::new(DNS_CACHE_TTL)));

    /// Return the global shared DNS resolver instance.
    pub fn shared_resolver() -> Arc<CachingResolver> {
        SHARED_RESOLVER.clone()
    }

    struct CacheEntry {
        addrs: Arc<Vec<SocketAddr>>,
        expires_at: Instant,
    }

    /// Per-hostname in-flight lookup cell.
    ///
    /// Uses `tokio::sync::OnceCell` so only the first concurrent request for
    /// a given hostname actually calls `getaddrinfo`; all others await the
    /// same future (single-flight / request coalescing).
    struct InflightEntry {
        cell: OnceCell<Arc<Vec<SocketAddr>>>,
    }

    /// A DNS resolver that caches results from the system resolver (`getaddrinfo`).
    ///
    /// Thread-safe and designed to be shared across all HTTP clients via `Arc`.
    ///
    /// Uses a two-layer design:
    /// - **Cache layer** (`Arc<Mutex<HashMap>>`): stores resolved addresses with a TTL.
    ///   Inner `Arc` is required because `Resolve::resolve` returns a `'static` future.
    /// - **Inflight layer** (`Arc<Mutex<HashMap<OnceCell>>>`): coalesces concurrent
    ///   lookups for the same hostname so `getaddrinfo` is called at most once per
    ///   hostname during a cold start (single-flight pattern).
    pub struct CachingResolver {
        cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
        inflight: Arc<Mutex<HashMap<String, Arc<InflightEntry>>>>,
        ttl: Duration,
    }

    impl CachingResolver {
        /// Create a new caching resolver with the given TTL for cache entries.
        pub(crate) fn new(ttl: Duration) -> Self {
            Self {
                cache: Arc::new(Mutex::new(HashMap::new())),
                inflight: Arc::new(Mutex::new(HashMap::new())),
                ttl,
            }
        }
    }

    impl Resolve for CachingResolver {
        fn resolve(&self, name: Name) -> Resolving {
            let hostname = name.as_str().to_string();
            let ttl = self.ttl;

            // Fast path: check cache.
            // SocketAddr is Copy and we typically have 1-4 addrs per host,
            // so cloning the small Vec is cheaper than an Arc indirection
            // through the iterator.
            {
                let cache = self.cache.lock();
                if let Some(entry) = cache.get(&hostname) {
                    if entry.expires_at > Instant::now() {
                        let cached = entry.addrs.to_vec();
                        return Box::pin(std::future::ready(Ok(
                            Box::new(cached.into_iter()) as Addrs
                        )));
                    }
                }
            }

            // Cache miss or expired: get-or-create an inflight entry so only
            // one task performs the actual DNS lookup (single-flight).
            let inflight = {
                let mut map = self.inflight.lock();
                map.entry(hostname.clone())
                    .or_insert_with(|| {
                        Arc::new(InflightEntry {
                            cell: OnceCell::new(),
                        })
                    })
                    .clone()
            };

            // Clone Arcs for the 'static async block (required by Resolve trait).
            let cache = Arc::clone(&self.cache);
            let inflight_map = Arc::clone(&self.inflight);

            Box::pin(async move {
                let resolved = inflight
                    .cell
                    .get_or_try_init(|| async {
                        let result: Vec<SocketAddr> =
                            tokio::net::lookup_host((hostname.as_str(), 0))
                                .await
                                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                                    Box::new(e)
                                })?
                                .collect();

                        if result.is_empty() {
                            tracing::warn!("DNS lookup returned no addresses for {}", hostname);
                        }

                        let addrs = Arc::new(result);

                        // Populate the cache if we got addresses
                        if !addrs.is_empty() {
                            let mut c = cache.lock();
                            c.insert(
                                hostname.clone(),
                                CacheEntry {
                                    addrs: Arc::clone(&addrs),
                                    expires_at: Instant::now() + ttl,
                                },
                            );
                        }

                        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(addrs)
                    })
                    .await?;

                // Clone out of the OnceCell before dropping inflight.
                // SocketAddr is Copy and we typically have 1-4 addrs, so
                // the small Vec allocation is negligible.
                let owned = resolved.to_vec();

                // Clean up the inflight entry
                {
                    let mut map = inflight_map.lock();
                    map.remove(&hostname);
                }

                let addrs: Addrs = Box::new(owned.into_iter());
                Ok(addrs)
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
