//! Per-run manifest store for the demand resolver.
//!
//! Holds the resolved-manifest cache, the edges waiting on each pending fetch,
//! and recorded failures — one slot per manifest kind. Pure storage with no
//! scheduling (that lives in [`super::queue`]) and no fetch orchestration (that
//! lives in the driver, which owns both this store and the queue).

use std::collections::{HashMap, VecDeque};
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

/// The version-manifest slot: resolved entries, edges parked on them, and
/// failures, keyed by package name then spec.
///
/// Nested maps on purpose: `select_edge` probes this slot for **every edge**
/// (`get_version_manifest` / `get_version_failure` / `is_version_settled`),
/// and a flat `HashMap<(String, String), _>` would force two `String`
/// allocations per probe just to build the key. Two-level `get` with
/// `(&str, &str)` allocates nothing.
///
/// Invariant: a key is in `cache` *or* `failures`, never both — the driver
/// fetches each key once (single-flight) and records exactly one outcome.
/// Maintenance invariant: removal helpers drop emptied inner maps so the
/// outer `is_empty` checks stay meaningful.
#[derive(Default)]
struct VersionSlot {
    cache: HashMap<String, HashMap<String, Arc<CoreVersionManifest>>>,
    waiters: HashMap<String, HashMap<String, Vec<WaitingEdge>>>,
    failures: HashMap<String, HashMap<String, String>>,
}

impl VersionSlot {
    fn get(&self, name: &str, spec: &str) -> Option<&Arc<CoreVersionManifest>> {
        self.cache.get(name)?.get(spec)
    }

    fn failure(&self, name: &str, spec: &str) -> Option<&str> {
        self.failures.get(name)?.get(spec).map(String::as_str)
    }

    /// Already resolved or failed — no need to fetch again.
    fn is_settled(&self, name: &str, spec: &str) -> bool {
        self.cache.get(name).is_some_and(|m| m.contains_key(spec))
            || self
                .failures
                .get(name)
                .is_some_and(|m| m.contains_key(spec))
    }

    /// Move every edge waiting on `(name, spec)` into `pending` so they retry.
    fn wake(&mut self, name: &str, spec: &str, pending: &mut VecDeque<WaitingEdge>) {
        if let Some(by_spec) = self.waiters.get_mut(name) {
            if let Some(waiters) = by_spec.remove(spec) {
                pending.extend(waiters);
            }
            if by_spec.is_empty() {
                self.waiters.remove(name);
            }
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
    /// Resolved version manifests, keyed by name then spec.
    version: VersionSlot,
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
                .flat_map(|(name, by_spec)| {
                    by_spec
                        .into_iter()
                        .map(move |(spec, manifest)| (name.clone(), spec, manifest))
                })
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
        let (name, spec) = key;
        self.version
            .waiters
            .entry(name)
            .or_default()
            .entry(spec)
            .or_default()
            .push(waiter);
    }

    /// Whether any edge is still parked on a pending package or version fetch.
    pub(crate) fn has_pending_waiters(&self) -> bool {
        !self.package_waiters.is_empty() || !self.version.waiters.is_empty()
    }

    /// Parked-edge counts as `(package, version)`, for diagnostics.
    pub(crate) fn pending_waiter_counts(&self) -> (usize, usize) {
        let package = self.package_waiters.values().map(Vec::len).sum();
        let version = self
            .version
            .waiters
            .values()
            .flat_map(HashMap::values)
            .map(Vec::len)
            .sum();
        (package, version)
    }

    /// Drain every parked edge (package + version waiters) for last-resort
    /// sequential resolution.
    pub(crate) fn drain_waiters(&mut self) -> Vec<WaitingEdge> {
        let mut all = Vec::new();
        for (_, waiters) in self.package_waiters.drain() {
            all.extend(waiters);
        }
        for (_, by_spec) in self.version.waiters.drain() {
            for (_, waiters) in by_spec {
                all.extend(waiters);
            }
        }
        all
    }

    /// Move every edge waiting on `name`'s package fetch into `ready`.
    pub(crate) fn wake_package(&mut self, name: &str, ready: &mut VecDeque<WaitingEdge>) {
        if let Some(waiters) = self.package_waiters.remove(name) {
            ready.extend(waiters);
        }
    }

    /// Look up a cached version manifest by `(name, spec)`. Zero-allocation:
    /// this runs for every edge the demand loop selects.
    pub(crate) fn get_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Option<Arc<CoreVersionManifest>> {
        self.version.get(name, spec).cloned()
    }

    /// Look up a recorded fetch failure for `(name, spec)`.
    pub(crate) fn get_version_failure(&self, name: &str, spec: &str) -> Option<&str> {
        self.version.failure(name, spec)
    }

    /// Whether `(name, spec)` is already resolved or failed — no refetch needed.
    pub(crate) fn is_version_settled(&self, name: &str, spec: &str) -> bool {
        self.version.is_settled(name, spec)
    }

    /// Record a fetch failure for `(name, spec)`.
    pub(crate) fn fail_version(&mut self, name: &str, spec: &str, error: String) {
        self.version
            .failures
            .entry(name.to_string())
            .or_default()
            .insert(spec.to_string(), error);
    }

    /// Move every edge waiting on `(name, spec)` into `ready` so it retries.
    pub(crate) fn wake_version(
        &mut self,
        name: &str,
        spec: &str,
        ready: &mut VecDeque<WaitingEdge>,
    ) {
        self.version.wake(name, spec, ready);
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
        let by_spec = self.version.cache.entry(name).or_default();
        by_spec.insert(spec, Arc::clone(&manifest));
        by_spec.entry(version).or_insert(manifest);
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
