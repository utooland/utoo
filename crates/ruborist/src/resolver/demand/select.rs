//! Per-edge resolution decisions over the manifest store — no I/O, no graph
//! mutation. The testable core of the driver: given the store and an edge,
//! decide whether to resolve now, skip, fail, or park + fetch.

use std::sync::Arc;

use crate::model::manifest::{CoreVersionManifest, FullManifest, VersionsRef};
use crate::model::node::EdgeType;
use crate::resolver::edges::DependencyEdgeInfo;
use crate::resolver::registry::ResolveError;
use crate::resolver::version::resolve_target_version_lazy;

use super::state::{ManifestState, PackageVersions};

fn resolve_version_from_versions<'a, RE>(
    edge: &DependencyEdgeInfo,
    package_name: &str,
    versions: VersionsRef<'_>,
    sorted: impl FnOnce() -> &'a [deno_semver::Version],
    real_spec: &str,
) -> Result<Option<String>, ResolveError<RE>> {
    if versions.versions.is_empty() {
        if edge.edge_type == EdgeType::Optional {
            return Ok(None);
        }
        return Err(ResolveError::NoVersions(package_name.to_string()));
    }

    let version = match resolve_target_version_lazy(versions, sorted, real_spec) {
        Ok(version) => version,
        Err(_) if edge.edge_type == EdgeType::Optional => return Ok(None),
        Err(e) => {
            return Err(ResolveError::Version(format!(
                "{}@{}: {}",
                edge.name, real_spec, e
            )));
        }
    };
    Ok(Some(version))
}

/// What to do with one registry dependency edge, decided from the current store
/// without mutating it. The caller applies the side effects (cache alias,
/// parking the waiter, enqueueing the fetch), so this stays a pure, testable
/// decision over the store.
pub(super) enum EdgeStep {
    /// Manifest already in the store — resolve the edge now. `alias` is an extra
    /// `(name, spec)` key to cache the manifest under first (so a manifest found
    /// by its resolved version is also reachable by the requested spec).
    Resolve {
        manifest: Arc<CoreVersionManifest>,
        alias: Option<(String, String)>,
    },
    /// Optional dependency with no matching/available version — skip it.
    Skip,
    /// A recorded fetch failure; the caller skips (optional) or errors.
    Fail(String),
    /// Park the edge on `wait`, then enqueue `fetch` to wake it later.
    Park { wait: WaitKey, fetch: FetchPlan },
}

/// Which waiter list a parked edge joins.
pub(super) enum WaitKey {
    Full(String),
    Version((String, String)),
}

/// The fetch to enqueue for a parked edge.
pub(super) enum FetchPlan {
    /// `schedule_registry_fetch` — a full or version job depending on the mode.
    Registry { name: String, spec: String },
    /// Extract an exact version from an already-fetched full manifest.
    Extract {
        name: String,
        version: String,
        full: Arc<FullManifest>,
    },
    /// Fetch an exact version manifest directly.
    VersionFetch { name: String, version: String },
}

/// Whether a resolved exact version is extracted from a cached full manifest or
/// fetched directly.
enum ExactFetch {
    Extract(Arc<FullManifest>),
    Version,
}

/// Terminal outcome when `(name, lookup_key)` is already settled in the store:
/// a recorded failure -> [`EdgeStep::Fail`], or a cached manifest ->
/// [`EdgeStep::Resolve`]. `spec` is the edge's requested spec; it's used for the
/// failure message and, when the lookup key is a resolved version
/// (`lookup_key != spec`), to alias the manifest back under the spec.
fn settled_step(
    state: &ManifestState,
    name: &str,
    lookup_key: &str,
    spec: &str,
) -> Option<EdgeStep> {
    if let Some(error) = state.get_version_failure(name, lookup_key) {
        return Some(EdgeStep::Fail(format!("{name}@{spec}: {error}")));
    }
    state
        .get_version_manifest(name, lookup_key)
        .map(|manifest| EdgeStep::Resolve {
            manifest,
            alias: (lookup_key != spec).then(|| (name.to_string(), spec.to_string())),
        })
}

/// Decide what to do with `edge` given the current store. `Err` is a fatal
/// version-resolution error; `Ok(EdgeStep::Fail)` is a recorded fetch failure
/// the caller treats as skip-or-error by dependency kind.
pub(super) fn select_edge<RE>(
    state: &ManifestState,
    edge: &DependencyEdgeInfo,
    name: &str,
    spec: &str,
    mode: ResolutionMode,
) -> Result<EdgeStep, ResolveError<RE>> {
    match mode {
        // Semver registries resolve the spec server-side, so a cached/failed
        // spec settles the edge; otherwise fetch the version manifest directly.
        ResolutionMode::Semver => {
            Ok(
                settled_step(state, name, spec, spec).unwrap_or_else(|| EdgeStep::Park {
                    wait: WaitKey::Version((name.to_string(), spec.to_string())),
                    fetch: FetchPlan::Registry {
                        name: name.to_string(),
                        spec: spec.to_string(),
                    },
                }),
            )
        }
        ResolutionMode::FullManifest => select_full_manifest::<RE>(state, edge, name, spec),
    }
}

/// Full-manifest mode: resolve the version client-side, falling through the
/// full-manifest cache, then the versions list, then a full fetch.
fn select_full_manifest<RE>(
    state: &ManifestState,
    edge: &DependencyEdgeInfo,
    name: &str,
    spec: &str,
) -> Result<EdgeStep, ResolveError<RE>> {
    // A cached or failed version for the spec settles the edge first — a cached
    // manifest still resolves even if the package's full fetch later failed.
    if let Some(step) = settled_step(state, name, spec, spec) {
        return Ok(step);
    }

    // Otherwise resolve a version client-side from the package's cached source.
    match state.package(name) {
        Some(PackageVersions::Failed(error)) => Ok(EdgeStep::Fail(format!("{name}: {error}"))),
        Some(PackageVersions::Full(full)) => {
            let full = Arc::clone(full);
            let Some(version) = resolve_version_from_versions::<RE>(
                edge,
                name,
                (&*full).into(),
                || full.sorted_parsed_versions(),
                spec,
            )?
            else {
                return Ok(EdgeStep::Skip);
            };
            Ok(select_resolved_version(
                state,
                name,
                spec,
                version,
                ExactFetch::Extract(full),
            ))
        }
        Some(PackageVersions::List(list)) => {
            let list = Arc::clone(list);
            let Some(version) = resolve_version_from_versions::<RE>(
                edge,
                name,
                (&*list).into(),
                || list.sorted_parsed_versions(),
                spec,
            )?
            else {
                return Ok(EdgeStep::Skip);
            };
            Ok(select_resolved_version(
                state,
                name,
                spec,
                version,
                ExactFetch::Version,
            ))
        }
        None => Ok(EdgeStep::Park {
            wait: WaitKey::Full(name.to_string()),
            fetch: FetchPlan::Registry {
                name: name.to_string(),
                spec: spec.to_string(),
            },
        }),
    }
}

/// Decide an edge whose exact `version` is already known (resolved
/// client-side). Shared by the full-manifest-cache and versions-list paths.
fn select_resolved_version(
    state: &ManifestState,
    name: &str,
    spec: &str,
    version: String,
    fetch: ExactFetch,
) -> EdgeStep {
    if let Some(step) = settled_step(state, name, &version, spec) {
        return step;
    }
    let fetch = match fetch {
        ExactFetch::Extract(full) => FetchPlan::Extract {
            name: name.to_string(),
            version: version.clone(),
            full,
        },
        ExactFetch::Version => FetchPlan::VersionFetch {
            name: name.to_string(),
            version: version.clone(),
        },
    };
    EdgeStep::Park {
        wait: WaitKey::Version((name.to_string(), version)),
        fetch,
    }
}

/// How a registry resolves versions: server-side semver vs full-manifest fetch.
#[derive(Clone, Copy)]
pub(crate) enum ResolutionMode {
    /// Registry resolves semver server-side — fetch version manifests directly.
    Semver,
    /// Fetch the full manifest and resolve versions client-side.
    FullManifest,
}

impl ResolutionMode {
    pub(crate) fn from_supports_semver(supports: bool) -> Self {
        if supports {
            ResolutionMode::Semver
        } else {
            ResolutionMode::FullManifest
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::graph::EdgeIndex;

    fn edge(name: &str, spec: &str, edge_type: EdgeType) -> DependencyEdgeInfo {
        DependencyEdgeInfo {
            edge_id: EdgeIndex::new(0),
            name: name.to_string(),
            spec: spec.to_string(),
            edge_type,
        }
    }

    fn version_manifest(name: &str, version: &str) -> Arc<CoreVersionManifest> {
        Arc::new(CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        })
    }

    fn full_manifest(name: &str, versions: &[&str]) -> Arc<FullManifest> {
        Arc::new(FullManifest {
            name: name.to_string(),
            versions: versions.iter().map(|v| v.to_string()).collect(),
            ..Default::default()
        })
    }

    fn select(state: &ManifestState, e: &DependencyEdgeInfo, mode: ResolutionMode) -> EdgeStep {
        select_edge::<()>(state, e, &e.name, &e.spec, mode).expect("no fatal version error")
    }

    #[test]
    fn semver_cache_hit_resolves_without_alias() {
        let mut state = ManifestState::default();
        state.cache_version(
            "pkg".into(),
            "^1.0.0".into(),
            version_manifest("pkg", "1.2.3"),
        );
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::Semver) {
            EdgeStep::Resolve { manifest, alias } => {
                assert_eq!(manifest.version, "1.2.3");
                assert!(alias.is_none());
            }
            _ => panic!("expected Resolve"),
        }
    }

    #[test]
    fn recorded_failure_returns_fail() {
        let mut state = ManifestState::default();
        state.fail_version("pkg", "^1.0.0", "boom".into());
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::Semver) {
            EdgeStep::Fail(msg) => assert!(msg.contains("boom")),
            _ => panic!("expected Fail"),
        }
    }

    #[test]
    fn semver_miss_parks_on_version_and_fetches_registry() {
        let state = ManifestState::default();
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::Semver) {
            EdgeStep::Park {
                wait: WaitKey::Version(key),
                fetch: FetchPlan::Registry { name, spec },
            } => {
                assert_eq!(key, ("pkg".into(), "^1.0.0".into()));
                assert_eq!((name.as_str(), spec.as_str()), ("pkg", "^1.0.0"));
            }
            _ => panic!("expected Park(Version, Registry)"),
        }
    }

    #[test]
    fn full_manifest_failure_returns_fail() {
        let mut state = ManifestState::default();
        state.set_package("pkg".into(), PackageVersions::Failed("gone".into()));
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::FullManifest) {
            EdgeStep::Fail(msg) => assert!(msg.contains("gone")),
            _ => panic!("expected Fail"),
        }
    }

    #[test]
    fn full_manifest_miss_parks_on_full() {
        let state = ManifestState::default();
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::FullManifest) {
            EdgeStep::Park {
                wait: WaitKey::Full(name),
                fetch: FetchPlan::Registry { .. },
            } => assert_eq!(name, "pkg"),
            _ => panic!("expected Park(Full, Registry)"),
        }
    }

    #[test]
    fn full_cache_resolves_version_then_parks_for_extract() {
        let mut state = ManifestState::default();
        state.set_package(
            "pkg".into(),
            PackageVersions::Full(full_manifest("pkg", &["1.2.3"])),
        );
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::FullManifest) {
            EdgeStep::Park {
                wait: WaitKey::Version(key),
                fetch: FetchPlan::Extract { version, .. },
            } => {
                assert_eq!(key, ("pkg".into(), "1.2.3".into()));
                assert_eq!(version, "1.2.3");
            }
            _ => panic!("expected Park(Version, Extract)"),
        }
    }

    #[test]
    fn optional_with_no_matching_version_skips() {
        let mut state = ManifestState::default();
        state.set_package(
            "pkg".into(),
            PackageVersions::Full(full_manifest("pkg", &["1.2.3"])),
        );
        let e = edge("pkg", "^9.0.0", EdgeType::Optional);
        match select(&state, &e, ResolutionMode::FullManifest) {
            EdgeStep::Skip => {}
            _ => panic!("expected Skip"),
        }
    }

    #[test]
    fn cached_version_resolves_despite_package_failure() {
        // A cached version manifest settles the edge even when the package's
        // full-manifest fetch failed (e.g. a warm-cache hit + a flaky refetch).
        let mut state = ManifestState::default();
        state.set_package(
            "pkg".into(),
            PackageVersions::Failed("full fetch boom".into()),
        );
        state.cache_version(
            "pkg".into(),
            "^1.0.0".into(),
            version_manifest("pkg", "1.2.3"),
        );
        let e = edge("pkg", "^1.0.0", EdgeType::Prod);
        match select(&state, &e, ResolutionMode::FullManifest) {
            EdgeStep::Resolve { manifest, .. } => assert_eq!(manifest.version, "1.2.3"),
            _ => panic!("expected Resolve"),
        }
    }
}
