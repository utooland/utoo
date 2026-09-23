//! Graph placement for resolved packages.
//!
//! "How a resolved manifest is attached to the graph": node creation, edge
//! reuse, the shared placement tail, and the chain-decorated error helper.
//! The natural neighbor of [`super::edges`]; consumed by both the spec router
//! in [`super::builder`] and the demand driver.

use std::sync::Arc;

use petgraph::graph::NodeIndex;

use super::builder::{BuildDepsConfig, ProcessResult, create_package_node};
use super::edges::{DependencyEdgeInfo, EdgeContext, add_edges_from};
use super::reuse::{
    ResolvedDependency, ReuseResult, find_identical_node, find_resolved_node, find_reusable_node,
};
use crate::model::graph::DependencyGraph;
use crate::model::node::DevDeps;
use crate::resolver::registry::ResolveError;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::ResolvedPackage;

/// Resolve `edge` onto an already-present compatible node by marking the edge
/// resolved. Shared by the pre-fetch reuse probe ([`try_reuse_dependency`]) and
/// the post-resolution placement ([`place_resolved_dependency`]), whose
/// `ReuseResult::Reuse` arms are otherwise identical. Node types are assigned in
/// a single pass after the tree is built (see [`compute_node_types`]).
pub(crate) fn reuse_existing_node(
    graph: &mut DependencyGraph,
    edge: &DependencyEdgeInfo,
    existing_index: NodeIndex,
) -> ProcessResult {
    graph.mark_dependency_resolved(edge.edge_id, existing_index);
    ProcessResult::Reused(existing_index)
}

pub(crate) fn try_reuse_dependency(
    graph: &mut DependencyGraph,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
) -> Option<ProcessResult> {
    match find_reusable_node(graph, parent, &edge.name, &edge.spec) {
        ReuseResult::Reuse(existing_index) => {
            Some(reuse_existing_node(graph, edge, existing_index))
        }
        ReuseResult::Install(_) => None,
    }
}

/// Compatibility entry point for callers that only supply a resolved package.
/// Keeps the existing request-based reuse followed by manifest identity matching.
/// Internal resolution uses `place_resolved_dependency` to retain the final
/// override target instead of inferring it from the original edge.
pub fn process_dependency_with_resolved(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    resolved: &ResolvedPackage,
    config: &BuildDepsConfig,
) -> ProcessResult {
    if let Some(reused) = try_reuse_dependency(graph, node_index, edge_info) {
        return reused;
    }
    match find_identical_node(graph, node_index, &edge_info.name, &resolved.manifest) {
        ReuseResult::Reuse(index) => reuse_existing_node(graph, edge_info, index),
        ReuseResult::Install(parent) => place_new_node(graph, parent, edge_info, resolved, config),
    }
}

pub(crate) fn place_resolved_dependency(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    resolved: &ResolvedDependency<'_>,
    config: &BuildDepsConfig,
) -> ProcessResult {
    // Query again after resolution: another edge may have placed a candidate.
    // Use the final requirement, preserving range reuse without reapplying overrides.
    match find_resolved_node(
        graph,
        node_index,
        &edge_info.name,
        &resolved.spec,
        &resolved.manifest,
    ) {
        ReuseResult::Reuse(existing_index) => reuse_existing_node(graph, edge_info, existing_index),
        ReuseResult::Install(parent) => {
            let package =
                ResolvedPackage::from_manifest(&edge_info.name, Arc::clone(&resolved.manifest));
            place_new_node(graph, parent, edge_info, &package, config)
        }
    }
}

/// Attach a freshly resolved package under `conflict_parent`: create the
/// node, link it physically, resolve the originating edge, and queue its own
/// dependency edges. The single placement tail shared by the spec router
/// ([`process_dependency`]) and the demand path
/// ([`place_resolved_dependency`]).
pub(crate) fn place_new_node(
    graph: &mut DependencyGraph,
    conflict_parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    resolved: &ResolvedPackage,
    config: &BuildDepsConfig,
) -> ProcessResult {
    let new_node = create_package_node(&edge.name, resolved, conflict_parent, graph);
    let new_index = graph.add_node(new_node);
    graph.add_physical_edge(conflict_parent, new_index);
    graph.mark_dependency_resolved(edge.edge_id, new_index);
    add_edges_from(
        graph,
        new_index,
        &*resolved.manifest,
        &EdgeContext::new(config.peer_deps, DevDeps::Exclude),
    );
    ProcessResult::Created(new_index)
}

pub(crate) fn chain_err<E>(
    graph: &DependencyGraph,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    inner: ResolveError<E>,
) -> ResolveError<E> {
    let mut chain = graph.logical_ancestry(parent);
    chain.push((edge.name.clone(), edge.spec.clone()));
    ResolveError::WithChain {
        chain,
        source: Box::new(inner),
    }
}

/// Build the graph node for an already-resolved registry manifest (override
/// resolution is applied upstream by the demand loop, which owns the per-run
/// manifest cache). Emits the resolve event and links the node.
pub(crate) fn handle_resolved_registry_manifest<E>(
    graph: &mut DependencyGraph,
    receiver: &E,
    parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    resolved: ResolvedDependency<'_>,
    config: &BuildDepsConfig,
) -> ProcessResult
where
    E: EventReceiver,
{
    receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));
    place_resolved_dependency(graph, parent, edge, &resolved, config)
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;
