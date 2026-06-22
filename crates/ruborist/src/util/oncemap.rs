//! OnceMap: A concurrent map that ensures each key's work is done exactly once.
//!
//! Based on the pattern from uv package manager:
//! <https://codepointer.substack.com/p/uv-oncemap-rust-pattern-for-running>
//!
//! When multiple tasks request the same resource concurrently:
//! - First caller executes the work
//! - Other callers wait and receive the shared result
//! - No duplicate work is performed
//!
//! # Typical Use Case in Package Manager
//!
//! ```text
//!                    Dependency Graph
//!
//!           ┌─────────┐
//!           │  app    │
//!           └────┬────┘
//!         ┌──────┼──────┐
//!         ▼      ▼      ▼
//!     ┌───────┐ ┌───┐ ┌───────┐
//!     │ lib-a │ │...│ │ lib-z │
//!     └───┬───┘ └───┘ └───┬───┘
//!         │               │
//!         └───────┬───────┘
//!                 ▼
//!            ┌─────────┐
//!            │  react  │  ◄── requested by multiple deps
//!            └─────────┘
//!
//!     Without OnceMap:
//!       lib-a fetches react ──► network request
//!       lib-z fetches react ──► network request (duplicate!)
//!
//!     With OnceMap:
//!       lib-a fetches react ──► network request
//!       lib-z waits on lib-a ──► shares result (no duplicate!)
//! ```
//!
//! # Failure semantics
//!
//! When `init` returns `Err`, the entry is removed and waiters wake up to
//! retry as fresh workers (since the original error can't be cloned across
//! waiters and we'd rather have each caller see a fresh error than a stale
//! shared one). A single transient failure isn't broadcast as a shared error
//! to every concurrent caller.

use dashmap::{DashMap, mapref::entry::Entry};
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::Notify;

/// The state of a value in the OnceMap.
enum Value<V> {
    /// Work is in progress, waiters can subscribe to the notify.
    Waiting(Arc<Notify>),
    /// Work is complete, result is available.
    Done(Arc<V>),
}

/// A concurrent map that ensures each key's work is done exactly once.
///
/// # Example
/// ```ignore
/// let map: OnceMap<String, Vec<u8>> = OnceMap::new();
///
/// // Multiple tasks can call get_or_try_init concurrently.
/// // Only one will actually fetch, others will wait.
/// let result = map.get_or_try_init("react", || async {
///     fetch_package("react").await
/// }).await;
/// ```
pub struct OnceMap<K, V> {
    map: DashMap<K, Value<V>>,
}

impl<K, V> std::fmt::Debug for OnceMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnceMap").finish_non_exhaustive()
    }
}

impl<K, V> Default for OnceMap<K, V>
where
    K: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> OnceMap<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Create a new empty OnceMap.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Get or initialize a value for the given key, single-flight.
    ///
    /// If the key doesn't exist, the provided async closure computes the
    /// value. If another task is already computing it, this waits for that
    /// worker. On `Err` the entry is removed and each waiter resumes by
    /// re-running its own `init` closure.
    pub async fn get_or_try_init<E, F, Fut>(&self, key: K, init: F) -> Result<Arc<V>, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>>,
    {
        // Fast path: check if already done
        if let Some(entry) = self.map.get(&key)
            && let Value::Done(result) = entry.value()
        {
            return Ok(Arc::clone(result));
        }

        // Try to register as the worker
        let notify = Arc::new(Notify::new());
        let entry = self.map.entry(key.clone());

        let is_worker = match entry {
            Entry::Occupied(occupied) => {
                match occupied.get() {
                    Value::Done(result) => {
                        return Ok(Arc::clone(result));
                    }
                    Value::Waiting(existing_notify) => {
                        let existing_notify = Arc::clone(existing_notify);

                        // Register the waiter BEFORE releasing the lock.
                        // This prevents a race condition where the worker
                        // completes and calls notify_waiters() between
                        // dropping the lock and registering the waiter -
                        // which would cause us to miss the notification and
                        // wait forever.
                        let notified = existing_notify.notified();
                        drop(occupied);

                        // Double-check: the value might have been inserted
                        // between releasing the lock and here.
                        if let Some(entry) = self.map.get(&key)
                            && let Value::Done(result) = entry.value()
                        {
                            return Ok(Arc::clone(result));
                        }

                        notified.await;

                        if let Some(entry) = self.map.get(&key)
                            && let Value::Done(result) = entry.value()
                        {
                            return Ok(Arc::clone(result));
                        }
                        // Worker failed; promote ourselves to retry as a fresh worker.
                        false
                    }
                }
            }
            Entry::Vacant(vacant) => {
                vacant.insert(Value::Waiting(Arc::clone(&notify)));
                true
            }
        };

        // If we observed a failed prior attempt, retry recursively. Recursion
        // depth is bounded by concurrent retries on the same key, which is
        // typically tiny; pinning avoids the boxed-future cost in the common
        // single-pass case.
        if !is_worker {
            return Box::pin(self.get_or_try_init(key, init)).await;
        }

        // Execute the work
        let result = init().await;

        match result {
            Ok(v) => {
                let arc = Arc::new(v);
                self.map.insert(key, Value::Done(Arc::clone(&arc)));
                notify.notify_waiters();
                Ok(arc)
            }
            Err(e) => {
                self.map.remove(&key);
                notify.notify_waiters();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::sync::Barrier;

    use super::*;

    #[tokio::test]
    async fn test_try_init_dedupes_success() {
        let map = Arc::new(OnceMap::<String, i32>::new());
        let call_count = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];
        for _ in 0..8 {
            let map = Arc::clone(&map);
            let call_count = Arc::clone(&call_count);
            handles.push(tokio::spawn(async move {
                map.get_or_try_init::<&'static str, _, _>("k".to_string(), || async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    call_count.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, &'static str>(7)
                })
                .await
            }));
        }
        for h in handles {
            assert_eq!(*h.await.unwrap().unwrap(), 7);
        }
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_try_init_failure_allows_retry() {
        let map: OnceMap<String, i32> = OnceMap::new();
        let result = map
            .get_or_try_init::<&'static str, _, _>("k".to_string(), || async move { Err("boom") })
            .await;
        assert_eq!(result.unwrap_err(), "boom");

        let result = map
            .get_or_try_init::<&'static str, _, _>("k".to_string(), || async move {
                Ok::<_, &'static str>(9)
            })
            .await;
        assert_eq!(*result.unwrap(), 9);
    }

    #[tokio::test]
    async fn test_try_init_waiter_retries_after_worker_error() {
        let map = Arc::new(OnceMap::<String, i32>::new());
        let attempts = Arc::new(AtomicUsize::new(0));

        let map1 = Arc::clone(&map);
        let attempts1 = Arc::clone(&attempts);
        let worker = tokio::spawn(async move {
            map1.get_or_try_init::<&'static str, _, _>("k".to_string(), || async move {
                attempts1.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                Err::<i32, _>("worker failed")
            })
            .await
        });

        let map2 = Arc::clone(&map);
        let attempts2 = Arc::clone(&attempts);
        let waiter = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            map2.get_or_try_init::<&'static str, _, _>("k".to_string(), || async move {
                attempts2.fetch_add(1, Ordering::SeqCst);
                Ok::<_, &'static str>(11)
            })
            .await
        });

        let (w, ww) = futures::future::join(worker, waiter).await;
        assert_eq!(w.unwrap().unwrap_err(), "worker failed");
        // Waiter sees the failure, promotes itself to a fresh worker, and succeeds.
        assert_eq!(*ww.unwrap().unwrap(), 11);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    /// Test that waiters don't miss notifications due to race conditions.
    ///
    /// Verifies that a waiter registers for the notification BEFORE releasing
    /// the map lock; otherwise the worker could complete and notify in the gap
    /// and the waiter would sleep forever.
    #[tokio::test]
    async fn test_no_missed_notifications() {
        let map = Arc::new(OnceMap::<String, i32>::new());
        let barrier = Arc::new(Barrier::new(2));

        let map_clone = Arc::clone(&map);
        let barrier_clone = Arc::clone(&barrier);
        let worker = tokio::spawn(async move {
            map_clone
                .get_or_try_init::<&'static str, _, _>("key".to_string(), || async move {
                    barrier_clone.wait().await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    Ok::<_, &'static str>(42)
                })
                .await
        });

        let map_clone = Arc::clone(&map);
        let barrier_clone = Arc::clone(&barrier);
        let waiter = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            barrier_clone.wait().await;
            map_clone
                .get_or_try_init::<&'static str, _, _>("key".to_string(), || async move {
                    panic!("Waiter should not execute work");
                })
                .await
        });

        let timeout = Duration::from_secs(2);
        let results = tokio::time::timeout(timeout, futures::future::join(worker, waiter))
            .await
            .expect("Test timed out - possible deadlock due to missed notification");

        assert_eq!(*results.0.unwrap().unwrap(), 42);
        assert_eq!(*results.1.unwrap().unwrap(), 42);
    }
}
