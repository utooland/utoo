//! Dependency reuse policy, separate from graph lookup and package placement.
//!
//! Before resolution, inspect the nearest candidate and the applicable override.
//! After resolution, use the final target without selecting override rules again.

use std::borrow::Cow;
use std::sync::Arc;

use petgraph::graph::NodeIndex;

use super::matching::{self, MatchResult};
use crate::model::graph::{DependencyGraph, DependencyLookup, FindResult, PackageNode};
use crate::model::manifest::CoreVersionManifest;

/// The final requirement and manifest, after applying any override.
/// Ordinary requests borrow their edge's spec; override targets own theirs.
pub(crate) struct ResolvedDependency<'a> {
    pub spec: Cow<'a, str>,
    pub manifest: Arc<CoreVersionManifest>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReuseResult {
    Reuse(NodeIndex),
    Install(NodeIndex),
}

enum ReusePhase<'a> {
    BeforeResolution,
    AfterResolution {
        requested_spec: &'a str,
        manifest: &'a CoreVersionManifest,
    },
}

impl DependencyGraph {
    /// Compatibility entry point for callers of the existing graph API.
    /// Reuse policy lives in the resolver; the graph only looks up candidates.
    pub fn find_compatible_node(&self, from: NodeIndex, name: &str, spec: &str) -> FindResult {
        match find_reusable_node(self, from, name, spec) {
            ReuseResult::Reuse(index) => FindResult::Reuse(index),
            ReuseResult::Install(parent) => match self.lookup_dependency(from, name) {
                DependencyLookup::Found(_) => FindResult::Conflict(parent),
                DependencyLookup::Missing { .. } => FindResult::New(parent),
            },
        }
    }
}

/// Try to reuse a candidate using only metadata already in the graph.
pub(crate) fn find_reusable_node(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
    spec: &str,
) -> ReuseResult {
    evaluate_candidate(
        graph,
        from,
        name,
        ReusePhase::BeforeResolution,
        |candidate| {
            let effective_spec = graph
                .check_override(from, name, None)
                .map_or(Cow::Borrowed(spec), Cow::Owned);
            matching::matches_spec(candidate, &effective_spec)
                && graph
                    .check_override(from, name, Some(&candidate.version))
                    .is_none_or(|target| {
                        matching::match_target(candidate, name, &target) == MatchResult::Match
                    })
        },
    )
}

/// Recheck candidate compatibility using the final effective requirement.
/// The original request only restricts where an invalidated slot can be replaced.
pub(crate) fn find_resolved_node(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
    requested_spec: &str,
    final_spec: &str,
    manifest: &CoreVersionManifest,
) -> ReuseResult {
    evaluate_candidate(
        graph,
        from,
        name,
        ReusePhase::AfterResolution {
            requested_spec,
            manifest,
        },
        |candidate| match matching::match_target(candidate, name, final_spec) {
            MatchResult::Match => true,
            MatchResult::NoMatch => false,
            MatchResult::NeedsResolution => {
                matching::matches_resolved_manifest(candidate, manifest)
            }
        },
    )
}

/// The legacy public placement API supplies a manifest without its final spec.
pub(crate) fn find_identical_node(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
    requested_spec: &str,
    manifest: &CoreVersionManifest,
) -> ReuseResult {
    evaluate_candidate(
        graph,
        from,
        name,
        ReusePhase::AfterResolution {
            requested_spec,
            manifest,
        },
        |candidate| matching::matches_resolved_manifest(candidate, manifest),
    )
}

fn evaluate_candidate(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
    phase: ReusePhase<'_>,
    accepts: impl FnOnce(&PackageNode) -> bool,
) -> ReuseResult {
    let candidate_index = match graph.lookup_dependency(from, name) {
        DependencyLookup::Found(index) => index,
        DependencyLookup::Missing { install_parent } => {
            return ReuseResult::Install(install_parent);
        }
    };
    let candidate = &graph.graph[candidate_index];
    if accepts(candidate) && graph.has_compatible_descendant_overrides(from, candidate_index) {
        return ReuseResult::Reuse(candidate_index);
    }
    // A changed override can vacate a locked slot. Once its replacement has
    // resolved, retain that slot only when its original request and descendant
    // rules have the same scope there as at the requester.
    if let ReusePhase::AfterResolution {
        requested_spec,
        manifest,
    } = phase
        && graph.is_invalidated_target(candidate_index)
        && let Some(parent) = graph.get_physical_parent(candidate_index)
        && graph.can_replace_override_target(from, parent, name, requested_spec, manifest)
        && !graph.reachable_nodes().contains(&candidate_index)
    {
        return ReuseResult::Install(parent);
    }
    // A nearer package shadows ancestors even when matching needs I/O.
    ReuseResult::Install(from)
}

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
