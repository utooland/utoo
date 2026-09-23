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
    evaluate_candidate(graph, from, name, |candidate| {
        let effective_spec = graph
            .check_override(from, name, None)
            .map_or(Cow::Borrowed(spec), Cow::Owned);
        matching::matches_spec(candidate, &effective_spec)
            && graph
                .check_override(from, name, Some(&candidate.version))
                .is_none_or(|target| {
                    matching::match_target(candidate, name, &target) == MatchResult::Match
                })
    })
}

/// Recheck the current graph using the final effective requirement.
/// This phase never selects overrides or checks the original edge's spec.
pub(crate) fn find_resolved_node(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
    spec: &str,
    manifest: &CoreVersionManifest,
) -> ReuseResult {
    evaluate_candidate(graph, from, name, |candidate| match matching::match_target(
        candidate, name, spec,
    ) {
        MatchResult::Match => true,
        MatchResult::NoMatch => false,
        MatchResult::NeedsResolution => matching::matches_resolved_manifest(candidate, manifest),
    })
}

/// The legacy public placement API supplies a manifest without its final spec.
pub(crate) fn find_identical_node(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
    manifest: &CoreVersionManifest,
) -> ReuseResult {
    evaluate_candidate(graph, from, name, |candidate| {
        matching::matches_resolved_manifest(candidate, manifest)
    })
}

fn evaluate_candidate(
    graph: &DependencyGraph,
    from: NodeIndex,
    name: &str,
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
        ReuseResult::Reuse(candidate_index)
    } else {
        // A nearer package shadows ancestors even when matching needs I/O.
        ReuseResult::Install(from)
    }
}

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
