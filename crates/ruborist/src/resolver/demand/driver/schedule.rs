//! Scheduling: decide which manifest jobs to enqueue (demand + speculative
//! prefetch) and push them onto the fetch queue, skipping anything the store
//! already has settled.

use std::sync::Arc;

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::{DevDeps, PeerDeps};
use crate::resolver::edges::DependencySource;
use crate::resolver::semver::normalize_spec;
use crate::service::{ManifestJob, MetadataFormat};
use crate::spec::SpecStr;

use super::super::queue::{FetchPriority, FetchQueues};
use super::super::state::ManifestState;
use super::plan::ResolutionMode;

fn version_metadata_format(supports_semver: ResolutionMode) -> MetadataFormat {
    if matches!(supports_semver, ResolutionMode::Semver) {
        MetadataFormat::Abbreviated
    } else {
        MetadataFormat::Complete
    }
}

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

/// Queue a registry fetch for `(name, spec)` unless the store already has it.
pub(super) fn schedule_registry_fetch(
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
                format: version_metadata_format(supports_semver),
            },
            priority,
        );
    } else {
        if state.full.is_settled(&real_name) || state.versions_cache.contains_key(&real_name) {
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

pub(super) fn enqueue_version_extract(
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

pub(super) fn enqueue_version_fetch(
    queues: &mut FetchQueues,
    name: String,
    fetch_spec: String,
    supports_semver: ResolutionMode,
) {
    queues.push(
        ManifestJob::Version {
            name,
            spec: fetch_spec.clone(),
            fetch_spec,
            format: version_metadata_format(supports_semver),
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
