//! OnceMap: A concurrent map that ensures each key's work is done exactly once.
//!
//! Based on the pattern from uv package manager:
//! https://codepointer.substack.com/p/uv-oncemap-rust-pattern-for-running
//!
//! When multiple tasks request the same resource concurrently:
//! - First caller executes the work
//! - Other callers wait and receive the shared result
//! - No duplicate work is performed

use dashmap::DashMap;
use std::future::Future;
use std::hash::Hash;
use std::pin::pin;
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
/// // Multiple tasks can call get_or_init concurrently
/// // Only one will actually fetch, others will wait
/// let result = map.get_or_init("react", || async {
///     fetch_package("react").await
/// }).await;
/// ```
pub struct OnceMap<K, V> {
    map: DashMap<K, Value<V>>,
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

    /// Get or initialize a value for the given key.
    ///
    /// If the key doesn't exist, the provided async closure will be called to compute the value.
    /// If another task is already computing the value, this will wait for it to complete.
    ///
    /// Returns `Some(Arc<V>)` if the computation succeeded, `None` if it failed.
    pub async fn get_or_init<F, Fut>(&self, key: K, init: F) -> Option<Arc<V>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<V>>,
    {
        // Fast path: check if already done
        if let Some(entry) = self.map.get(&key) {
            if let Value::Done(result) = entry.value() {
                return Some(Arc::clone(result));
            }
        }

        // Try to register as the worker
        let notify = Arc::new(Notify::new());
        let entry = self.map.entry(key.clone());

        match entry {
            dashmap::mapref::entry::Entry::Occupied(occupied) => {
                // Someone else is working on it or it's done
                match occupied.get() {
                    Value::Done(result) => {
                        return Some(Arc::clone(result));
                    }
                    Value::Waiting(existing_notify) => {
                        let existing_notify = Arc::clone(existing_notify);
                        drop(occupied);

                        // Wait for the worker to complete
                        // Use pin! to ensure the future is pinned before registering
                        let notified = pin!(existing_notify.notified());
                        notified.await;

                        // Check the result
                        if let Some(entry) = self.map.get(&key) {
                            if let Value::Done(result) = entry.value() {
                                return Some(Arc::clone(result));
                            }
                        }
                        return None;
                    }
                }
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                // We are the worker
                vacant.insert(Value::Waiting(Arc::clone(&notify)));
            }
        }

        // Execute the work
        let result = init().await;

        // Update the map with the result
        match result {
            Some(value) => {
                let arc_value = Arc::new(value);
                self.map.insert(key, Value::Done(Arc::clone(&arc_value)));
                notify.notify_waiters();
                Some(arc_value)
            }
            None => {
                // Work failed, remove the entry so others can retry
                self.map.remove(&key);
                notify.notify_waiters();
                None
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn test_single_execution() {
        let map: OnceMap<String, i32> = OnceMap::new();
        let call_count = Arc::new(AtomicUsize::new(0));

        let call_count_clone = Arc::clone(&call_count);
        let result = map
            .get_or_init("key".to_string(), || async move {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                Some(42)
            })
            .await;

        assert_eq!(*result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Second call should return cached result
        let call_count_clone = Arc::clone(&call_count);
        let result = map
            .get_or_init("key".to_string(), || async move {
                call_count_clone.fetch_add(1, Ordering::SeqCst);
                Some(100)
            })
            .await;

        assert_eq!(*result.unwrap(), 42); // Still 42, not 100
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // Still 1, not called again
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let map = Arc::new(OnceMap::<String, i32>::new());
        let call_count = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        // Spawn 10 concurrent tasks requesting the same key
        for _ in 0..10 {
            let map = Arc::clone(&map);
            let call_count = Arc::clone(&call_count);

            handles.push(tokio::spawn(async move {
                map.get_or_init("shared_key".to_string(), || async move {
                    // Simulate slow work
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    call_count.fetch_add(1, Ordering::SeqCst);
                    Some(42)
                })
                .await
            }));
        }

        // Wait for all tasks
        let results: Vec<_> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        // All should get the same result
        for result in results {
            assert_eq!(*result.unwrap(), 42);
        }

        // Work should only be done once
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_failed_work_allows_retry() {
        let map: OnceMap<String, i32> = OnceMap::new();
        let attempt = Arc::new(AtomicUsize::new(0));

        // First attempt fails
        let attempt_clone = Arc::clone(&attempt);
        let result = map
            .get_or_init("key".to_string(), || async move {
                attempt_clone.fetch_add(1, Ordering::SeqCst);
                None // Fail
            })
            .await;
        assert!(result.is_none());

        // Second attempt should be allowed and succeed
        let attempt_clone = Arc::clone(&attempt);
        let result = map
            .get_or_init("key".to_string(), || async move {
                attempt_clone.fetch_add(1, Ordering::SeqCst);
                Some(42)
            })
            .await;
        assert_eq!(*result.unwrap(), 42);
        assert_eq!(attempt.load(Ordering::SeqCst), 2);
    }
}
