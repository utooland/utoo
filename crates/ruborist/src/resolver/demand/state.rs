//! Per-run manifest store for the demand resolver.
//!
//! Holds the resolved-manifest cache, the edges waiting on each pending fetch,
//! and recorded failures — one slot per manifest kind. Pure storage with no
//! scheduling (that lives in [`super::queue`]) and no fetch orchestration (that
//! lives in the driver, which owns both this store and the queue).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;

use petgraph::graph::NodeIndex;

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::resolver::edges::DependencyEdgeInfo;
use crate::service::VersionsInfo;

/// A parked edge waiting on a pending fetch: its node and the dependency edge.
/// Defined here — the store owns the waiter payload — and shared by the driver.
pub(super) type WaitingEdge = (NodeIndex, DependencyEdgeInfo);

/// Resolved version manifests produced by one run, as neutral
/// `(name, spec, manifest)` tuples. The persistence layer (`ProjectCacheData`)
/// adapts these to/from disk; the store itself stays format-agnostic.
#[derive(Default)]
pub(crate) struct ResolverManifestCache {
    pub(crate) entries: Vec<(String, String, Arc<CoreVersionManifest>)>,
}

/// A manifest cache slot: resolved entries, edges waiting on them, and failures.
///
/// The resolver keeps one slot per manifest kind (full package manifests keyed
/// by name, version manifests keyed by `(name, spec)`), so the demand loop can
/// dedupe fetches, park waiters, and remember failures uniformly.
///
/// Invariant: a key is in `cache` *or* `failures`, never both — the driver
/// fetches each key once (single-flight) and records exactly one outcome.
pub(crate) struct ManifestSlot<K, V> {
    pub(crate) cache: HashMap<K, Arc<V>>,
    pub(crate) waiters: HashMap<K, Vec<WaitingEdge>>,
    pub(crate) failures: HashMap<K, String>,
}

// Manual `Default` so the slot does not require `K: Default`/`V: Default`.
impl<K, V> Default for ManifestSlot<K, V> {
    fn default() -> Self {
        Self {
            cache: HashMap::new(),
            waiters: HashMap::new(),
            failures: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone, V> ManifestSlot<K, V> {
    /// Already resolved or failed — no need to fetch again.
    pub(crate) fn is_settled(&self, key: &K) -> bool {
        self.cache.contains_key(key) || self.failures.contains_key(key)
    }

    /// Move every edge waiting on `key` into `pending` so they retry next pass.
    pub(crate) fn wake(&mut self, key: &K, pending: &mut VecDeque<WaitingEdge>) {
        if let Some(waiters) = self.waiters.remove(key) {
            pending.extend(waiters);
        }
    }
}

/// What the store knows about a package's available versions. A package has at
/// most one source at a time — its full-manifest fetch failed, or we have its
/// full manifest, or we have just its versions list (from a 304/abbreviated
/// response). Folding these into one enum keeps the "at most one" invariant in
/// the type and lets the resolver decide with a single lookup + `match`.
pub(crate) enum PackageVersions {
    /// The full-manifest fetch failed; no version of the package can resolve.
    Failed(String),
    /// Full manifest (all versions) — resolve a concrete version client-side.
    Full(Arc<FullManifest>),
    /// Versions list only — resolve a version, then fetch its manifest.
    List(Arc<VersionsInfo>),
}

/// The per-run manifest store. `packages` holds each package's version source
/// (failed / full manifest / versions list); `version` holds the resolved
/// per-version manifests. Read/written by the driver through the methods below;
/// scheduling lives separately (see [`super::queue`]).
#[derive(Default)]
pub(crate) struct ManifestState {
    /// Per-package version source/status, keyed by package name.
    pub(crate) packages: HashMap<String, PackageVersions>,
    /// Edges parked waiting on a package's full/versions fetch, keyed by name.
    pub(crate) package_waiters: HashMap<String, Vec<WaitingEdge>>,
    /// Resolved version manifests, keyed by `(name, spec)`.
    pub(crate) version: ManifestSlot<(String, String), CoreVersionManifest>,
}

impl ManifestState {
    /// Build a store pre-seeded with already-resolved `(name, spec, manifest)`
    /// entries (e.g. from a warm project cache). The caller adapts whatever
    /// persistence format it has into these neutral tuples.
    pub(crate) fn seeded(entries: Vec<(String, String, Arc<CoreVersionManifest>)>) -> Self {
        let mut state = Self::default();
        for (name, spec, manifest) in entries {
            state.cache_version(name, spec, manifest);
        }
        state
    }

    /// Drain the resolved version manifests as neutral tuples (for persistence).
    pub(crate) fn into_resolver_cache(self) -> ResolverManifestCache {
        ResolverManifestCache {
            entries: self
                .version
                .cache
                .into_iter()
                .map(|((name, spec), manifest)| (name, spec, manifest))
                .collect(),
        }
    }

    /// The cached version source for `name`, if any.
    pub(crate) fn package(&self, name: &str) -> Option<&PackageVersions> {
        self.packages.get(name)
    }

    /// Whether a version source (full manifest, versions list, or failure) is
    /// already known for `name`, so its full manifest need not be re-fetched.
    pub(crate) fn has_package_source(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    /// Record a package's version source.
    pub(crate) fn set_package(&mut self, name: String, source: PackageVersions) {
        self.packages.insert(name, source);
    }

    /// Park an edge waiting on `name`'s package (full/versions) fetch.
    pub(crate) fn park_on_package(&mut self, name: String, waiter: WaitingEdge) {
        self.package_waiters.entry(name).or_default().push(waiter);
    }

    /// Park an edge waiting on a `(name, spec)` version-manifest fetch.
    pub(crate) fn park_on_version(&mut self, key: (String, String), waiter: WaitingEdge) {
        self.version.waiters.entry(key).or_default().push(waiter);
    }

    /// Whether any edge is still parked on a pending package or version fetch.
    pub(crate) fn has_pending_waiters(&self) -> bool {
        !self.package_waiters.is_empty() || !self.version.waiters.is_empty()
    }

    /// Parked-edge counts as `(package, version)`, for diagnostics.
    pub(crate) fn pending_waiter_counts(&self) -> (usize, usize) {
        let package = self.package_waiters.values().map(Vec::len).sum();
        let version = self.version.waiters.values().map(Vec::len).sum();
        (package, version)
    }

    /// Drain every parked edge (package + version waiters) for last-resort
    /// sequential resolution.
    pub(crate) fn drain_waiters(&mut self) -> Vec<WaitingEdge> {
        let mut all = Vec::new();
        for (_, waiters) in self.package_waiters.drain() {
            all.extend(waiters);
        }
        for (_, waiters) in self.version.waiters.drain() {
            all.extend(waiters);
        }
        all
    }

    /// Move every edge waiting on `name`'s package fetch into `ready`.
    pub(crate) fn wake_package(&mut self, name: &str, ready: &mut VecDeque<WaitingEdge>) {
        if let Some(waiters) = self.package_waiters.remove(name) {
            ready.extend(waiters);
        }
    }

    /// Look up a cached version manifest by `(name, spec)`.
    pub(crate) fn get_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Option<Arc<CoreVersionManifest>> {
        self.version
            .cache
            .get(&(name.to_string(), spec.to_string()))
            .cloned()
    }

    /// Look up a recorded fetch failure for `(name, spec)`.
    pub(crate) fn get_version_failure(&self, name: &str, spec: &str) -> Option<&str> {
        self.version
            .failures
            .get(&(name.to_string(), spec.to_string()))
            .map(String::as_str)
    }

    /// Whether `(name, spec)` is already resolved or failed — no refetch needed.
    pub(crate) fn is_version_settled(&self, name: &str, spec: &str) -> bool {
        self.version
            .is_settled(&(name.to_string(), spec.to_string()))
    }

    /// Record a fetch failure for `(name, spec)`.
    pub(crate) fn fail_version(&mut self, name: &str, spec: &str, error: String) {
        self.version
            .failures
            .insert((name.to_string(), spec.to_string()), error);
    }

    /// Move every edge waiting on `(name, spec)` into `ready` so it retries.
    pub(crate) fn wake_version(
        &mut self,
        name: &str,
        spec: &str,
        ready: &mut VecDeque<WaitingEdge>,
    ) {
        self.version
            .wake(&(name.to_string(), spec.to_string()), ready);
    }

    /// Cache a version manifest under both its requested spec and its resolved
    /// version, so later lookups by either key hit memory.
    pub(crate) fn cache_version(
        &mut self,
        name: String,
        spec: String,
        manifest: Arc<CoreVersionManifest>,
    ) {
        let version = manifest.version.clone();
        self.version
            .cache
            .insert((name.clone(), spec), Arc::clone(&manifest));
        self.version
            .cache
            .entry((name, version))
            .or_insert(manifest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, version: &str) -> Arc<CoreVersionManifest> {
        Arc::new(CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn test_cache_version_indexes_by_spec_and_version() {
        let mut state = ManifestState::default();
        state.cache_version(
            "pkg".to_string(),
            "^1.0.0".to_string(),
            manifest("pkg", "1.2.3"),
        );
        // Reachable by both the requested spec and the resolved version, and it
        // is the manifest we stored — not merely *some* entry.
        assert_eq!(
            state.get_version_manifest("pkg", "^1.0.0").unwrap().version,
            "1.2.3"
        );
        assert_eq!(
            state.get_version_manifest("pkg", "1.2.3").unwrap().version,
            "1.2.3"
        );
        assert!(state.get_version_manifest("pkg", "^9.0.0").is_none());
    }

    #[test]
    fn test_cache_version_collapses_when_spec_equals_version() {
        // An exact-version spec keys both inserts to the same slot → one entry.
        let mut state = ManifestState::default();
        state.cache_version(
            "pkg".to_string(),
            "1.2.3".to_string(),
            manifest("pkg", "1.2.3"),
        );
        assert_eq!(
            state.get_version_manifest("pkg", "1.2.3").unwrap().version,
            "1.2.3"
        );
        assert_eq!(state.into_resolver_cache().entries.len(), 1);
    }

    #[test]
    fn test_seeded_preloads_entries() {
        let state = ManifestState::seeded(vec![(
            "a".to_string(),
            "^1".to_string(),
            manifest("a", "1.0.0"),
        )]);
        assert_eq!(
            state.get_version_manifest("a", "^1").unwrap().version,
            "1.0.0"
        );
        assert_eq!(
            state.get_version_manifest("a", "1.0.0").unwrap().version,
            "1.0.0"
        );
    }

    #[test]
    fn test_fail_version_records_and_settles() {
        let mut state = ManifestState::default();
        assert!(!state.is_version_settled("pkg", "^1"));
        state.fail_version("pkg", "^1", "boom".to_string());
        assert_eq!(state.get_version_failure("pkg", "^1"), Some("boom"));
        assert!(state.is_version_settled("pkg", "^1"));
    }

    #[test]
    fn test_into_resolver_cache_drains_both_keys() {
        let mut state = ManifestState::default();
        state.cache_version(
            "pkg".to_string(),
            "^1.0.0".to_string(),
            manifest("pkg", "1.2.3"),
        );
        // Drains both the requested-spec and resolved-version entries.
        let mut specs: Vec<String> = state
            .into_resolver_cache()
            .entries
            .into_iter()
            .map(|(_, spec, _)| spec)
            .collect();
        specs.sort();
        assert_eq!(specs, ["1.2.3", "^1.0.0"]);
    }
}
