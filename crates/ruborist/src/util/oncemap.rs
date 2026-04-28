#![allow(dead_code)]
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
/// // Multiple tasks can call get_or_init concurrently
/// // Only one will actually fetch, others will wait
/// let result = map.get_or_init("react", || async {
///     fetch_package("react").await
/// }).await;
/// ```
pub struct OnceMap<K, V> {
    map: DashMap<K, Value<V>>,
    /// Waiters for keys that don't exist yet.
    /// When get_or_init/complete finishes a key, it notifies and removes the entry.
    waiters: DashMap<K, Arc<Notify>>,
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
            waiters: DashMap::new(),
        }
    }

    /// Register a key as pending (Waiting state) without starting work.
    /// Returns the Notify handle if newly registered, None if already exists.
    pub fn register(&self, key: K) -> Option<Arc<Notify>> {
        use dashmap::mapref::entry::Entry;
        match self.map.entry(key) {
            Entry::Occupied(_) => None,
            Entry::Vacant(vacant) => {
                let notify = Arc::new(Notify::new());
                vacant.insert(Value::Waiting(Arc::clone(&notify)));
                Some(notify)
            }
        }
    }

    /// Wait for a key to complete.
    ///
    /// - If the key exists and is Done, returns immediately.
    /// - If the key exists and is Waiting, waits for it to complete.
    /// - If the key doesn't exist yet, waits for it to be created and completed.
    ///
    /// This enables parent-ordering in the clone pipeline: a child package
    /// can wait for its parent's clone to finish even if the parent's
    /// `get_or_init` hasn't been called yet.
    pub async fn wait_if_pending(&self, key: &K) {
        // Fast path: key already exists
        if let Some(entry) = self.map.get(key) {
            match entry.value() {
                Value::Done(_) => return,
                Value::Waiting(notify) => {
                    let notify = Arc::clone(notify);
                    let notified = notify.notified();
                    drop(entry);

                    if let Some(entry) = self.map.get(key)
                        && matches!(entry.value(), Value::Done(_))
                    {
                        return;
                    }

                    notified.await;
                    return;
                }
            }
        }

        // Key doesn't exist yet — register a creation waiter.
        // get_or_init/complete will notify this when the key reaches Done (or fails).
        let creation_notify = self
            .waiters
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone();

        // Register notified BEFORE double-checking to prevent missed notifications.
        let notified = creation_notify.notified();

        // Double-check: key might have been created and completed between
        // the initial map.get and registering here.
        if let Some(entry) = self.map.get(key)
            && matches!(entry.value(), Value::Done(_))
        {
            return;
        }

        // Wait for the key to be created and completed
        notified.await;
    }

    /// Complete a pre-registered key with a value.
    ///
    /// This is used in conjunction with `register()` for the pre-registration pattern:
    /// 1. Call `register(key)` synchronously to claim the key
    /// 2. Do async work
    /// 3. Call `complete(key, value, notify)` to store result and notify waiters
    ///
    /// The notify handle must be the one returned from `register()`.
    pub fn complete(&self, key: K, value: Option<V>, notify: Arc<Notify>) {
        match value {
            Some(v) => {
                self.map.insert(key.clone(), Value::Done(Arc::new(v)));
            }
            None => {
                self.map.remove(&key);
            }
        }
        self.notify_all(&key, &notify);
    }

    /// Notify both the work-notify and any creation waiters for a key.
    fn notify_all(&self, key: &K, notify: &Notify) {
        notify.notify_waiters();
        if let Some((_, cn)) = self.waiters.remove(key) {
            cn.notify_waiters();
        }
    }

    /// Get or initialize a value for the given key.
    ///
    /// If the key doesn't exist, the provided async closure will be called to compute the value.
    /// If another task is already computing the value, this will wait for it to complete.
    ///
    /// Returns `Some(Arc<V>)` if the computation succeeded, `None` if it failed.
    ///
    /// For fallible work that needs to propagate a typed error to the worker
    /// caller, see [`Self::get_or_try_init`].
    pub async fn get_or_init<F, Fut>(&self, key: K, init: F) -> Option<Arc<V>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<V>>,
    {
        // Fast path: check if already done
        if let Some(entry) = self.map.get(&key)
            && let Value::Done(result) = entry.value()
        {
            return Some(Arc::clone(result));
        }

        // Try to register as the worker
        let notify = Arc::new(Notify::new());
        let entry = self.map.entry(key.clone());

        match entry {
            Entry::Occupied(occupied) => {
                // Someone else is working on it or it's done
                match occupied.get() {
                    Value::Done(result) => {
                        return Some(Arc::clone(result));
                    }
                    Value::Waiting(existing_notify) => {
                        let existing_notify = Arc::clone(existing_notify);

                        // Register the waiter BEFORE releasing the lock.
                        // This prevents a race condition where the worker completes
                        // and calls notify_waiters() between dropping the lock and
                        // registering the waiter - which would cause us to miss the
                        // notification and wait forever.
                        let notified = existing_notify.notified();

                        // Now safe to release the lock
                        drop(occupied);

                        // Double-check: the value might have been inserted between
                        // releasing the lock and here. If so, return it directly.
                        if let Some(entry) = self.map.get(&key)
                            && let Value::Done(result) = entry.value()
                        {
                            return Some(Arc::clone(result));
                        }

                        // Wait for the worker to complete
                        notified.await;

                        // Check the result
                        if let Some(entry) = self.map.get(&key)
                            && let Value::Done(result) = entry.value()
                        {
                            return Some(Arc::clone(result));
                        }
                        return None;
                    }
                }
            }
            Entry::Vacant(vacant) => {
                // We are the worker
                vacant.insert(Value::Waiting(Arc::clone(&notify)));
            }
        }

        // Execute the work
        let result = init().await;

        // Update the map with the result
        let arc_value = result.map(|v| {
            let arc = Arc::new(v);
            self.map.insert(key.clone(), Value::Done(Arc::clone(&arc)));
            arc
        });
        if arc_value.is_none() {
            self.map.remove(&key);
        }
        self.notify_all(&key, &notify);
        arc_value
    }

    /// Like [`Self::get_or_init`] but the work returns a `Result`. The worker
    /// caller receives `Err(E)` if the closure failed; waiters see `Ok(None)`
    /// (the key is cleared, next caller retries the work).
    ///
    /// On success, both the worker and waiters get `Ok(Some(Arc<V>))`.
    pub async fn get_or_try_init<E, F, Fut>(&self, key: K, init: F) -> Result<Option<Arc<V>>, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>>,
    {
        // Fast path: already done.
        if let Some(entry) = self.map.get(&key)
            && let Value::Done(result) = entry.value()
        {
            return Ok(Some(Arc::clone(result)));
        }

        let notify = Arc::new(Notify::new());
        let entry = self.map.entry(key.clone());

        match entry {
            Entry::Occupied(occupied) => match occupied.get() {
                Value::Done(result) => return Ok(Some(Arc::clone(result))),
                Value::Waiting(existing_notify) => {
                    let existing_notify = Arc::clone(existing_notify);
                    let notified = existing_notify.notified();
                    drop(occupied);

                    if let Some(entry) = self.map.get(&key)
                        && let Value::Done(result) = entry.value()
                    {
                        return Ok(Some(Arc::clone(result)));
                    }

                    notified.await;

                    if let Some(entry) = self.map.get(&key)
                        && let Value::Done(result) = entry.value()
                    {
                        return Ok(Some(Arc::clone(result)));
                    }
                    return Ok(None);
                }
            },
            Entry::Vacant(vacant) => {
                vacant.insert(Value::Waiting(Arc::clone(&notify)));
            }
        }

        let result = init().await;
        match result {
            Ok(v) => {
                let arc = Arc::new(v);
                self.map.insert(key.clone(), Value::Done(Arc::clone(&arc)));
                self.notify_all(&key, &notify);
                Ok(Some(arc))
            }
            Err(e) => {
                self.map.remove(&key);
                self.notify_all(&key, &notify);
                Err(e)
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

    /// Test that waiters don't miss notifications due to race conditions.
    ///
    /// This test verifies the fix for a race condition where a waiter could
    /// miss the notification if the worker completed between the waiter
    /// releasing the lock and registering for notifications.
    ///
    /// Timeline of the race condition (before fix):
    /// ```text
    /// Worker                          Waiter
    /// ──────                          ──────
    /// 1. register key
    ///    insert Waiting(notify)
    ///    start work...
    ///                                 2. sees Waiting state
    ///                                    clone notify
    ///                                    drop(lock) ← release lock
    ///
    ///                                    ┌─────────────────┐
    ///                                    │  DANGER WINDOW  │
    ///                                    │  (not listening │
    ///                                    │   for notify)   │
    ///                                    └─────────────────┘
    ///
    /// 3. work done!
    ///    insert Done(value)
    ///    notify_waiters() ← sent!
    ///    (but no one listening)
    ///
    ///                                 4. notified = notify.notified()
    ///                                    ← too late! already notified
    ///
    ///                                 5. notified.await
    ///                                    ← waits forever, deadlock!
    /// ```
    ///
    /// The fix: register `notified()` BEFORE releasing the lock, then
    /// double-check if the value was inserted.
    #[tokio::test]
    async fn test_no_missed_notifications() {
        use tokio::sync::Barrier;

        let map = Arc::new(OnceMap::<String, i32>::new());
        let barrier = Arc::new(Barrier::new(2));

        // Spawn the worker task
        let map_clone = Arc::clone(&map);
        let barrier_clone = Arc::clone(&barrier);
        let worker = tokio::spawn(async move {
            map_clone
                .get_or_init("key".to_string(), || async move {
                    // Wait for waiter to be ready
                    barrier_clone.wait().await;
                    // Small delay to let waiter release lock
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    Some(42)
                })
                .await
        });

        // Spawn waiter task
        let map_clone = Arc::clone(&map);
        let barrier_clone = Arc::clone(&barrier);
        let waiter = tokio::spawn(async move {
            // Small delay to ensure worker registers first
            tokio::time::sleep(Duration::from_millis(1)).await;

            // Signal worker to proceed
            barrier_clone.wait().await;

            // This should not hang - if it does, the race condition exists
            map_clone
                .get_or_init("key".to_string(), || async move {
                    panic!("Waiter should not execute work");
                })
                .await
        });

        // Use timeout to detect deadlock
        let timeout = Duration::from_secs(2);
        let results = tokio::time::timeout(timeout, futures::future::join(worker, waiter))
            .await
            .expect("Test timed out - possible deadlock due to missed notification");

        // Both should get the same result
        assert_eq!(*results.0.unwrap().unwrap(), 42);
        assert_eq!(*results.1.unwrap().unwrap(), 42);
    }
}
