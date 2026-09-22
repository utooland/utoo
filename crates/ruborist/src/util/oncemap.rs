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
//! When `init` fails, is canceled, or panics, its entry is removed and waiters wake up to
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

/// Owns one initialization generation. Dropping a canceled/panicking future
/// releases its claim; identity checking prevents an old owner from removing
/// a newer attempt for the same key.
struct Initializer<'a, K: Eq + Hash, V> {
    map: &'a DashMap<K, Value<V>>,
    key: K,
    owner: Arc<Notify>,
}

impl<K: Eq + Hash, V> Drop for Initializer<'_, K, V> {
    fn drop(&mut self) {
        self.map.remove_if(
            &self.key,
            |_, value| matches!(value, Value::Waiting(owner) if Arc::ptr_eq(owner, &self.owner)),
        );
        self.owner.notify_waiters();
    }
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
        loop {
            let (owner, waiting) = match self.map.entry(key.clone()) {
                Entry::Occupied(entry) => match entry.get() {
                    Value::Done(value) => return Ok(Arc::clone(value)),
                    Value::Waiting(owner) => {
                        let owner = Arc::clone(owner);
                        // Snapshot the notification generation before releasing
                        // the map lock, including failure/cancellation wakeups.
                        let waiting = Arc::clone(&owner).notified_owned();
                        (owner, Some(waiting))
                    }
                },
                Entry::Vacant(entry) => {
                    let owner = Arc::new(Notify::new());
                    entry.insert(Value::Waiting(Arc::clone(&owner)));
                    (owner, None)
                }
            };
            if let Some(waiting) = waiting {
                waiting.await;
                continue;
            }
            let guard = Initializer {
                map: &self.map,
                key: key.clone(),
                owner,
            };
            let value = Arc::new(init().await?);
            if let Entry::Occupied(mut entry) = self.map.entry(key)
                && matches!(entry.get(), Value::Waiting(owner) if Arc::ptr_eq(owner, &guard.owner))
            {
                entry.insert(Value::Done(Arc::clone(&value)));
            }
            // Dropping the guard wakes waiters and keeps a completed value.
            return Ok(value);
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
    async fn canceled_owner_wakes_registered_waiters() {
        let map = Arc::new(OnceMap::<String, u32>::new());
        let (started, ready) = tokio::sync::oneshot::channel();
        let owner_map = Arc::clone(&map);
        let owner = tokio::spawn(async move {
            owner_map
                .get_or_try_init::<(), _, _>("k".into(), || async {
                    let _ = started.send(());
                    std::future::pending().await
                })
                .await
        });
        ready.await.unwrap();
        let mut waiter =
            std::pin::pin!(map.get_or_try_init::<(), _, _>("k".into(), || async { Ok(42) }));
        assert!(futures::poll!(waiter.as_mut()).is_pending());
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        let result = tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*result, 42);
    }

    #[tokio::test]
    async fn panicking_owner_wakes_waiters_and_allows_retry() {
        let map = Arc::new(OnceMap::<String, u32>::new());
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let owner_map = Arc::clone(&map);
        let owner = tokio::spawn(async move {
            owner_map
                .get_or_try_init::<(), _, _>("k".into(), || async {
                    let _ = started.send(());
                    released.await.unwrap();
                    panic!("initializer panic");
                })
                .await
        });
        ready.await.unwrap();
        let mut waiter =
            std::pin::pin!(map.get_or_try_init::<(), _, _>("k".into(), || async { Ok(7) }));
        assert!(futures::poll!(waiter.as_mut()).is_pending());
        release.send(()).unwrap();
        assert!(owner.await.unwrap_err().is_panic());
        let result = tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*result, 7);
    }

    #[test]
    fn old_owner_cannot_remove_new_initialization() {
        let map = OnceMap::<String, u32>::new();
        let owner = Arc::new(Notify::new());
        let guard = Initializer {
            map: &map.map,
            key: "k".into(),
            owner: Arc::clone(&owner),
        };
        map.map.insert("k".into(), Value::Waiting(owner));
        let replacement = Arc::new(Notify::new());
        map.map
            .insert("k".into(), Value::Waiting(Arc::clone(&replacement)));
        drop(guard);
        assert!(
            matches!(map.map.get("k").unwrap().value(), Value::Waiting(current) if Arc::ptr_eq(current, &replacement))
        );
    }

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
