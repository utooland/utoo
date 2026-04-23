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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, LazyLock};
    use std::time::{Duration, Instant};

    use anyhow::Result;
    use parking_lot::Mutex;
    use reqwest::dns::{Addrs, Name, Resolve, Resolving};
    use tokio::sync::OnceCell;

    /// Fixed fallback TTL for cached DNS entries.
    ///
    /// Ideally we'd respect the TTL from DNS records, but `getaddrinfo`
    /// (used by `tokio::net::lookup_host`) does not expose it — the OS
    /// resolver abstracts that away. Getting real TTL would require a raw
    /// DNS library like `hickory-dns`, which is incompatible with sandboxed
    /// environments (the primary reason this module exists).
    ///
    /// 300 s is a reasonable default for a CLI tool that typically runs for
    /// seconds to minutes.
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
        /// Round-robin counter used to rotate the returned address order
        /// on every `resolve` call. Spreads concurrent pool connections
        /// across all DNS A records (e.g. antgroup registry returns 8
        /// IPs) instead of pinning every new connection to the first IP
        /// like reqwest does by default.
        rr_counter: AtomicUsize,
    }

    impl CachingResolver {
        /// Create a new caching resolver with the given TTL for cache entries.
        pub(crate) fn new(ttl: Duration) -> Self {
            Self {
                cache: Arc::new(Mutex::new(HashMap::new())),
                inflight: Arc::new(Mutex::new(HashMap::new())),
                ttl,
                rr_counter: AtomicUsize::new(0),
            }
        }
    }

    /// Rotate `addrs` left by `offset` positions, producing a fresh Vec so
    /// reqwest's connect loop tries a different IP on every new connection.
    ///
    /// e.g. `[A, B, C, D]` with offset 2 → `[C, D, A, B]`.
    fn rotate_addrs(addrs: &[SocketAddr], offset: usize) -> Vec<SocketAddr> {
        if addrs.is_empty() {
            return Vec::new();
        }
        let n = addrs.len();
        let start = offset % n;
        addrs[start..]
            .iter()
            .chain(&addrs[..start])
            .copied()
            .collect()
    }

    /// Perform a system DNS lookup and cache the result.
    async fn do_lookup(
        hostname: &str,
        cache: &Mutex<HashMap<String, CacheEntry>>,
        ttl: Duration,
    ) -> Result<Arc<Vec<SocketAddr>>> {
        // Returns both IPv4 and IPv6 addresses from the system resolver.
        // reqwest handles dual-stack via Happy Eyeballs (RFC 8305): it tries
        // IPv6 first, and starts a parallel IPv4 attempt after ~250 ms if
        // IPv6 hasn't connected yet. The first to succeed wins. This means
        // broken IPv6 adds at most ~250 ms latency on the first connection;
        // subsequent requests reuse the connection pool.
        let result: Vec<SocketAddr> = tokio::net::lookup_host((hostname, 0)).await?.collect();

        if result.is_empty() {
            tracing::warn!("DNS lookup returned no addresses for {}", hostname);
        }

        let addrs = Arc::new(result);

        // Populate the cache if we got addresses
        if !addrs.is_empty() {
            let mut c = cache.lock();
            c.insert(
                hostname.to_string(),
                CacheEntry {
                    addrs: Arc::clone(&addrs),
                    expires_at: Instant::now() + ttl,
                },
            );
        }

        Ok(addrs)
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
                if let Some(entry) = cache.get(&hostname)
                    && entry.expires_at > Instant::now()
                {
                    let offset = self.rr_counter.fetch_add(1, Ordering::Relaxed);
                    let rotated = rotate_addrs(&entry.addrs, offset);
                    return Box::pin(std::future::ready(Ok(
                        Box::new(rotated.into_iter()) as Addrs
                    )));
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
            let offset = self.rr_counter.fetch_add(1, Ordering::Relaxed);

            Box::pin(async move {
                let result = inflight
                    .cell
                    .get_or_try_init(|| do_lookup(&hostname, &cache, ttl))
                    .await;

                // Always clean up the inflight entry — even on error —
                // to prevent a memory leak if lookups consistently fail.
                inflight_map.lock().remove(&hostname);

                let resolved = result?;
                let rotated = rotate_addrs(resolved.as_ref(), offset);
                let addrs: Addrs = Box::new(rotated.into_iter());
                Ok(addrs)
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
