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
use crate::model::graph::{DependencyGraph, FindResult};
use crate::model::manifest::CoreVersionManifest;
use crate::model::node::DevDeps;
use crate::resolver::registry::ResolveError;
use crate::traits::progress::{BuildEvent, EventReceiver};
use crate::traits::registry::ResolvedPackage;

/// Resolve `edge` onto an already-present compatible node by marking the edge
/// resolved. Shared by the pre-fetch reuse probe ([`try_reuse_dependency`]) and
/// the post-resolution placement ([`process_dependency_with_resolved`]), whose
/// `FindResult::Reuse` arms are otherwise identical. Node types are assigned in
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
    match graph.find_compatible_node(parent, &edge.name, &edge.spec) {
        FindResult::Reuse(existing_index) => Some(reuse_existing_node(graph, edge, existing_index)),
        FindResult::Conflict(_) | FindResult::New(_) => None,
    }
}

pub fn process_dependency_with_resolved(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    edge_info: &DependencyEdgeInfo,
    resolved: &ResolvedPackage,
    config: &BuildDepsConfig,
) -> ProcessResult {
    // Other edges in this level may have filled a compatible slot while this
    // manifest was being fetched. Preserve ordinary range/alias reuse; only
    // conditional or source overrides need the exact resolved identity below.
    if let Some(reused) = try_reuse_dependency(graph, node_index, edge_info) {
        return reused;
    }
    match graph.find_resolved_node(node_index, &edge_info.name, &resolved.manifest) {
        FindResult::Reuse(existing_index) => reuse_existing_node(graph, edge_info, existing_index),
        FindResult::Conflict(conflict_parent) | FindResult::New(conflict_parent) => {
            place_new_node(graph, conflict_parent, edge_info, resolved, config)
        }
    }
}

/// Attach a freshly resolved package under `conflict_parent`: create the
/// node, link it physically, resolve the originating edge, and queue its own
/// dependency edges. The single placement tail shared by the spec router
/// ([`process_dependency`]) and the demand path
/// ([`process_dependency_with_resolved`]).
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
    manifest: Arc<CoreVersionManifest>,
    config: &BuildDepsConfig,
) -> ProcessResult
where
    E: EventReceiver,
{
    let resolved = ResolvedPackage::from_manifest(edge.name.clone(), manifest);
    receiver.on_event(BuildEvent::PackageResolved((&*resolved.manifest).into()));
    process_dependency_with_resolved(graph, parent, edge, &resolved, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::graph::PackageNode;
    use crate::model::node::EdgeType;
    use crate::model::package_json::PackageJson;
    use std::path::PathBuf;

    #[test]
    fn fetched_manifest_reuses_a_compatible_slot_including_aliases() {
        for real_name in ["ms", "raw-body"] {
            let mut graph = DependencyGraph::from_package_json(
                PathBuf::from("/project"),
                PackageJson::new("root", "1.0.0"),
            );
            let root = graph.root_index;
            let existing = graph.add_node(PackageNode::from_version_manifest(
                "ms".into(),
                PathBuf::from("/project/node_modules/ms"),
                Arc::new(CoreVersionManifest {
                    name: real_name.into(),
                    version: "2.1.3".into(),
                    ..Default::default()
                }),
            ));
            graph.add_physical_edge(root, existing);
            let edge = DependencyEdgeInfo {
                edge_id: graph.add_dependency_edge(root, "ms", "^2.1.3", EdgeType::Prod),
                name: "ms".into(),
                spec: "^2.1.3".into(),
                edge_type: EdgeType::Prod,
            };
            let resolved = ResolvedPackage::from_manifest(
                "ms",
                Arc::new(CoreVersionManifest {
                    name: "ms".into(),
                    version: "2.1.4".into(),
                    ..Default::default()
                }),
            );
            assert!(matches!(
                process_dependency_with_resolved(&mut graph, root, &edge, &resolved, &BuildDepsConfig::default()),
                ProcessResult::Reused(index) if index == existing
            ));
            assert_eq!(graph.graph.node_count(), 2);
        }
    }
}
