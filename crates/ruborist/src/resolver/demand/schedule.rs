//! Fetch-scheduling policy for the demand resolver.
//!
//! Decides *what manifest jobs to enqueue* — the dual of [`super::select`],
//! which decides what to do with an edge. Given an edge's resolution plan or a
//! freshly resolved manifest, these helpers translate it into [`ManifestJob`]s
//! and push them onto the [`FetchQueues`] scheduler, de-duplicating against the
//! [`ManifestState`] store so each `(name, spec)` is fetched at most once.
//!
//! The driver owns the loop and the in-flight set; this module owns the
//! "fetch next" policy it delegates to.

use std::sync::Arc;

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::{DevDeps, PeerDeps};
use crate::resolver::edges::DependencySource;
use crate::resolver::semver::normalize_spec;
use crate::service::ManifestJob;
use crate::spec::SpecStr;

use super::queue::{FetchPriority, FetchQueues};
use super::select::{FetchPlan, ResolutionMode};
use super::state::ManifestState;

/// Collect a manifest's registry dependencies (excluding dev deps) as
/// `(name, spec)` pairs eligible for speculative prefetch.
fn collect_registry_prefetches(
    manifest: &CoreVersionManifest,
    peer_deps: PeerDeps,
) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    manifest.for_each_dep(peer_deps, DevDeps::Exclude, |_, name, spec| {
        if spec.is_registry_spec() {
            deps.push((name.to_string(), spec.to_string()));
        }
    });
    deps
}

/// Schedule the fetch a parked edge is waiting on. Owns the [`FetchPlan`] →
/// [`ManifestJob`] mapping so the driver only says "schedule this plan" — parked
/// edges are always demand priority.
pub(super) fn schedule_fetch(
    state: &mut ManifestState,
    queues: &mut FetchQueues,
    plan: FetchPlan,
    supports_semver: ResolutionMode,
) {
    match plan {
        FetchPlan::Registry { name, spec } => schedule_registry_fetch(
            state,
            queues,
            name,
            spec,
            supports_semver,
            FetchPriority::Demand,
        ),
        FetchPlan::Extract {
            name,
            version,
            full,
        } => enqueue_version_extract(queues, name, version, full),
        FetchPlan::VersionFetch { name, version } => enqueue_version_fetch(queues, name, version),
    }
}

/// Queue a registry fetch for `(name, spec)` unless the store already has it.
fn schedule_registry_fetch(
    state: &mut ManifestState,
    queues: &mut FetchQueues,
    name: String,
    spec: String,
    supports_semver: ResolutionMode,
    priority: FetchPriority,
) {
    let (real_name, real_spec) = normalize_spec(&name, &spec);
    if matches!(supports_semver, ResolutionMode::Semver) {
        if state.is_version_settled(&real_name, &real_spec) {
            return;
        }
        queues.push(
            ManifestJob::Version {
                name: real_name.clone(),
                spec: real_spec.clone(),
                fetch_spec: real_spec,
            },
            priority,
        );
    } else {
        if state.has_package_source(&real_name) {
            return;
        }
        queues.push(
            ManifestJob::Full {
                name: real_name,
                spec: Some(real_spec),
            },
            priority,
        );
    }
}

fn enqueue_version_extract(
    queues: &mut FetchQueues,
    name: String,
    version: String,
    full: Arc<FullManifest>,
) {
    queues.push(
        ManifestJob::ExtractVersion {
            name,
            spec: version.clone(),
            version,
            full,
        },
        FetchPriority::Demand,
    );
}

fn enqueue_version_fetch(queues: &mut FetchQueues, name: String, fetch_spec: String) {
    queues.push(
        ManifestJob::Version {
            name,
            spec: fetch_spec.clone(),
            fetch_spec,
        },
        FetchPriority::Demand,
    );
}

pub(super) fn schedule_transitive_prefetches(
    state: &mut ManifestState,
    queues: &mut FetchQueues,
    manifest: &CoreVersionManifest,
    peer_deps: PeerDeps,
    supports_semver: ResolutionMode,
) {
    for (name, spec) in collect_registry_prefetches(manifest, peer_deps) {
        schedule_registry_fetch(
            state,
            queues,
            name,
            spec,
            supports_semver,
            FetchPriority::Prefetch,
        );
    }
}
