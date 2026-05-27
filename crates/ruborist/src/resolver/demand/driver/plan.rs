//! Per-edge resolution decisions over the manifest store — no I/O, no graph
//! mutation. The testable core of the driver: given the store and an edge,
//! decide whether to resolve now, skip, fail, or park + fetch.

use std::sync::Arc;

use crate::model::manifest::{CoreVersionManifest, FullManifest, VersionsRef};
use crate::model::node::EdgeType;
use crate::resolver::edges::DependencyEdgeInfo;
use crate::resolver::registry::ResolveError;
use crate::resolver::version::resolve_target_version;

use super::super::state::ManifestState;

fn resolve_version_from_versions<RE>(
    edge: &DependencyEdgeInfo,
    package_name: &str,
    versions: VersionsRef<'_>,
    real_spec: &str,
) -> Result<Option<String>, ResolveError<RE>> {
    if versions.versions.is_empty() {
        if edge.edge_type == EdgeType::Optional {
            return Ok(None);
        }
        return Err(ResolveError::NoVersions(package_name.to_string()));
    }

    let version = match resolve_target_version(versions, real_spec) {
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

fn resolve_version_from_full_manifest<RE>(
    edge: &DependencyEdgeInfo,
    full: &FullManifest,
    real_spec: &str,
) -> Result<Option<String>, ResolveError<RE>> {
    resolve_version_from_versions(edge, &full.name, full.into(), real_spec)
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

/// Decide what to do with `edge` given the current store. `Err` is a fatal
/// version-resolution error; `Ok(EdgeStep::Fail)` is a recorded fetch failure
/// the caller treats as skip-or-error by dependency kind.
pub(super) fn plan_edge<RE>(
    state: &ManifestState,
    edge: &DependencyEdgeInfo,
    name: &str,
    spec: &str,
    mode: ResolutionMode,
) -> Result<EdgeStep, ResolveError<RE>> {
    match mode {
        ResolutionMode::Semver => {
            // The registry resolves the spec server-side: fetch it directly.
            if let Some(error) = state.get_version_failure(name, spec) {
                return Ok(EdgeStep::Fail(format!("{name}@{spec}: {error}")));
            }
            if let Some(manifest) = state.get_version_manifest(name, spec) {
                return Ok(EdgeStep::Resolve {
                    manifest,
                    alias: None,
                });
            }
            Ok(EdgeStep::Park {
                wait: WaitKey::Version((name.to_string(), spec.to_string())),
                fetch: FetchPlan::Registry {
                    name: name.to_string(),
                    spec: spec.to_string(),
                },
            })
        }
        ResolutionMode::FullManifest => plan_edge_full_manifest::<RE>(state, edge, name, spec),
    }
}

/// Full-manifest mode: resolve the version client-side, falling through the
/// full-manifest cache, then the versions list, then a full fetch.
fn plan_edge_full_manifest<RE>(
    state: &ManifestState,
    edge: &DependencyEdgeInfo,
    name: &str,
    spec: &str,
) -> Result<EdgeStep, ResolveError<RE>> {
    if let Some(error) = state.full.failures.get(name) {
        return Ok(EdgeStep::Fail(format!("{name}: {error}")));
    }
    if let Some(error) = state.get_version_failure(name, spec) {
        return Ok(EdgeStep::Fail(format!("{name}@{spec}: {error}")));
    }
    if let Some(manifest) = state.get_version_manifest(name, spec) {
        return Ok(EdgeStep::Resolve {
            manifest,
            alias: None,
        });
    }

    if let Some(full) = state.full.cache.get(name).cloned() {
        let Some(version) = resolve_version_from_full_manifest::<RE>(edge, &full, spec)? else {
            return Ok(EdgeStep::Skip);
        };
        return plan_resolved_version::<RE>(state, name, spec, version, ExactFetch::Extract(full));
    }

    if let Some(versions) = state.versions_cache.get(name).cloned() {
        let Some(version) =
            resolve_version_from_versions::<RE>(edge, name, (&*versions).into(), spec)?
        else {
            return Ok(EdgeStep::Skip);
        };
        return plan_resolved_version::<RE>(state, name, spec, version, ExactFetch::Version);
    }

    Ok(EdgeStep::Park {
        wait: WaitKey::Full(name.to_string()),
        fetch: FetchPlan::Registry {
            name: name.to_string(),
            spec: spec.to_string(),
        },
    })
}

/// Decide an edge whose exact `version` is already known (resolved
/// client-side). Shared by the full-manifest-cache and versions-list paths.
fn plan_resolved_version<RE>(
    state: &ManifestState,
    name: &str,
    spec: &str,
    version: String,
    fetch: ExactFetch,
) -> Result<EdgeStep, ResolveError<RE>> {
    if let Some(error) = state.get_version_failure(name, &version) {
        return Ok(EdgeStep::Fail(format!("{name}@{spec}: {error}")));
    }
    if let Some(manifest) = state.get_version_manifest(name, &version) {
        return Ok(EdgeStep::Resolve {
            manifest,
            alias: Some((name.to_string(), spec.to_string())),
        });
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
    Ok(EdgeStep::Park {
        wait: WaitKey::Version((name.to_string(), version)),
        fetch,
    })
}

/// How a registry resolves versions: server-side semver vs full-manifest fetch.
/// Replaces a bare `supports_semver: bool` threaded through the resolve loop.
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
