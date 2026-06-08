//! Fetch scheduling for the demand resolver.
//!
//! Two-tier job queue with single-flight de-duplication. Each manifest is
//! fetched at most once even if several edges request it; on-demand fetches
//! (an edge is blocked on them) are always dispatched ahead of speculative
//! prefetches. Pure scheduling — holds no manifest data (that's [`super::state`])
//! and does not depend on it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::model::manifest::CoreVersionManifest;
use crate::service::{ManifestFullData, ManifestJob};

/// Identifies a manifest fetch for de-duplication and waiter bookkeeping.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FetchKey {
    Full(String),
    Version(String, String),
}

impl ManifestJob {
    pub(crate) fn key(&self) -> FetchKey {
        match self {
            Self::Full { name, .. } => FetchKey::Full(name.clone()),
            Self::Version { name, spec, .. } | Self::ExtractVersion { name, spec, .. } => {
                FetchKey::Version(name.clone(), spec.clone())
            }
        }
    }
}

/// Result of one completed fetch job, handed back to the main loop.
pub(crate) enum FetchDone {
    Full {
        name: String,
        result: Result<ManifestFullData, String>,
    },
    Version {
        name: String,
        spec: String,
        result: Result<Arc<CoreVersionManifest>, String>,
    },
}

impl FetchDone {
    pub(crate) fn key(&self) -> FetchKey {
        match self {
            Self::Full { name, .. } => FetchKey::Full(name.clone()),
            Self::Version { name, spec, .. } => FetchKey::Version(name.clone(), spec.clone()),
        }
    }
}

/// A fetch job owned by the demand driver. Native wraps a `tokio::spawn`
/// handle so independent fetch + parse jobs progress on the multi-threaded
/// runtime; wasm keeps the provider future local and lets `FuturesUnordered`
/// poll browser-backed I/O without requiring a Tokio `LocalSet`.
pub(crate) type FetchFuture = Pin<Box<dyn Future<Output = Result<FetchDone, String>>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FetchPriority {
    Demand,
    Prefetch,
}

/// Demand/prefetch job queues plus the bookkeeping that keeps fetches
/// single-flight (`queued` = waiting to start, `active` = in flight) and demand
/// ahead of prefetch.
#[derive(Default)]
pub(crate) struct FetchQueues {
    demand: VecDeque<ManifestJob>,
    prefetch: VecDeque<ManifestJob>,
    pub(crate) queued: HashMap<FetchKey, FetchPriority>,
    pub(crate) active: HashSet<FetchKey>,
}

impl FetchQueues {
    /// Queue a fetch. Skips manifests already in flight; a prefetch already
    /// queued is promoted to demand if re-requested on demand.
    pub(crate) fn push(&mut self, request: ManifestJob, priority: FetchPriority) {
        let key = request.key();
        if self.active.contains(&key) {
            return;
        }

        match (self.queued.get(&key).copied(), priority) {
            (Some(FetchPriority::Demand), _)
            | (Some(FetchPriority::Prefetch), FetchPriority::Prefetch) => {}
            (Some(FetchPriority::Prefetch), FetchPriority::Demand)
            | (None, FetchPriority::Demand) => {
                self.queued.insert(key, FetchPriority::Demand);
                self.demand.push_back(request);
            }
            (None, FetchPriority::Prefetch) => {
                self.queued.insert(key, FetchPriority::Prefetch);
                self.prefetch.push_back(request);
            }
        }
    }

    pub(crate) fn complete(&mut self, key: &FetchKey) {
        self.queued.remove(key);
        self.active.remove(key);
    }

    /// Take the next fetch — demand first, then prefetch — marking it in flight.
    pub(crate) fn pop(&mut self) -> Option<ManifestJob> {
        self.pop_priority(FetchPriority::Demand)
            .or_else(|| self.pop_priority(FetchPriority::Prefetch))
    }

    fn pop_priority(&mut self, priority: FetchPriority) -> Option<ManifestJob> {
        loop {
            let request = match priority {
                FetchPriority::Demand => self.demand.pop_front(),
                FetchPriority::Prefetch => self.prefetch.pop_front(),
            }?;
            let key = request.key();
            // Skip stale entries (e.g. a prefetch that was promoted to demand).
            if self.queued.get(&key).copied() != Some(priority) {
                continue;
            }
            self.queued.remove(&key);
            self.active.insert(key);
            return Some(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(name: &str) -> ManifestJob {
        ManifestJob::Full {
            name: name.to_string(),
            spec: None,
        }
    }

    #[test]
    fn test_pop_prioritizes_demand_over_prefetch() {
        let mut queues = FetchQueues::default();
        queues.push(full("prefetch"), FetchPriority::Prefetch);
        queues.push(
            ManifestJob::Version {
                name: "demand".to_string(),
                spec: "^1.0.0".to_string(),
                fetch_spec: "^1.0.0".to_string(),
            },
            FetchPriority::Demand,
        );

        assert_eq!(
            queues.pop().unwrap().key(),
            FetchKey::Version("demand".to_string(), "^1.0.0".to_string())
        );
        assert_eq!(
            queues.pop().unwrap().key(),
            FetchKey::Full("prefetch".to_string())
        );
        assert!(queues.pop().is_none());
    }

    #[test]
    fn test_push_promotes_prefetch_to_demand() {
        let mut queues = FetchQueues::default();
        queues.push(full("pkg"), FetchPriority::Prefetch);
        queues.push(full("pkg"), FetchPriority::Demand);

        let key = FetchKey::Full("pkg".to_string());
        assert_eq!(queues.queued.get(&key), Some(&FetchPriority::Demand));
        assert_eq!(queues.pop().unwrap().key(), key.clone());
        assert!(queues.active.contains(&key));
        // The stale prefetch entry left in the queue is skipped.
        assert!(queues.pop().is_none());
    }

    #[test]
    fn test_push_dedupes_in_flight_fetch() {
        let mut queues = FetchQueues::default();
        queues.push(full("pkg"), FetchPriority::Demand);
        let job = queues.pop().unwrap();
        // Same manifest requested again while in flight: ignored.
        queues.push(full("pkg"), FetchPriority::Demand);
        assert!(queues.pop().is_none());
        // After completion it can be queued again.
        queues.complete(&job.key());
        queues.push(full("pkg"), FetchPriority::Demand);
        assert!(queues.pop().is_some());
    }
}
