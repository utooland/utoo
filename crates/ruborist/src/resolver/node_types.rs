//! Node dependency-type propagation (prod/dev/optional/peer).
//!
//! A self-contained graph-analysis pass with no coupling to resolution: the
//! BFS only builds graph structure, and this pass — run once after the tree
//! is complete — is the single source of truth for node types.

use std::collections::{HashSet, VecDeque};

use petgraph::graph::NodeIndex;

use crate::model::graph::DependencyGraph;
use crate::model::node::EdgeType;

/// Snapshot of node dependency flags to avoid borrowing conflicts.
#[derive(Debug, Clone, Copy)]
struct NodeFlags {
    is_root: bool,
    is_prod: bool,
    is_dev: bool,
    is_optional: bool,
    is_peer: bool,
}

/// Update target node type based on source node and edge type.
///
/// This function propagates dependency types through the graph according to npm rules:
/// - Root dependencies directly set the target node type
/// - Prod dependencies propagate through prod edges
/// - Dev/Optional flags propagate only when appropriate
///
/// Returns whether the target node's flags actually changed — the fixpoint uses
/// this to decide whether to re-queue the target, without a second `get_node`
/// read to diff (the mutable borrow here already has the before/after values).
pub fn update_node_type_from_edge(
    graph: &mut DependencyGraph,
    from_index: NodeIndex,
    to_index: NodeIndex,
    edge_type: &EdgeType,
) -> bool {
    // Extract source node information to avoid borrowing conflicts
    let source_flags = {
        let from_node = graph
            .get_node(from_index)
            .expect("Source node must exist in graph");
        NodeFlags {
            is_root: from_node.is_root(),
            is_prod: from_node.is_prod,
            is_dev: from_node.is_dev,
            is_optional: from_node.is_optional,
            is_peer: from_node.is_peer,
        }
    };

    let to_node = graph
        .get_node_mut(to_index)
        .expect("Target node must exist in graph");
    let before = (
        to_node.is_prod,
        to_node.is_dev,
        to_node.is_optional,
        to_node.is_peer,
    );

    // Root node dependencies directly determine target type
    if source_flags.is_root {
        match edge_type {
            EdgeType::Prod => {
                to_node.is_prod = true;
                to_node.is_dev = false;
                to_node.is_optional = false;
                to_node.is_peer = false;
            }
            EdgeType::Dev => {
                if !to_node.is_prod {
                    to_node.is_dev = true;
                    to_node.is_optional = false;
                }
            }
            EdgeType::Optional => {
                if !to_node.is_prod && !to_node.is_dev {
                    to_node.is_optional = true;
                }
            }
            EdgeType::Peer => {
                if !to_node.is_prod && !to_node.is_dev {
                    to_node.is_peer = true;
                }
            }
        }
    } else {
        // Propagate types from non-root nodes
        // 1. Source's dev status propagates to target
        // 2. If edge is Optional, target gets optional flag (unless already prod)
        if source_flags.is_dev && !to_node.is_prod {
            to_node.is_dev = true;
        }

        // Handle edge type
        if *edge_type == EdgeType::Optional && !to_node.is_prod {
            // Optional edge -> target is optional
            to_node.is_optional = true;
        } else if source_flags.is_prod && *edge_type != EdgeType::Optional {
            // Prod source with non-optional edge -> target becomes prod
            to_node.is_prod = true;
            to_node.is_dev = false;
            to_node.is_optional = false;
            to_node.is_peer = false;
        } else if source_flags.is_optional && !to_node.is_prod {
            // Optional source propagates optional status
            // Note: don't check !is_dev here - devOptional packages should propagate both flags
            to_node.is_optional = true;
        } else if source_flags.is_peer && !to_node.is_prod && !to_node.is_dev {
            to_node.is_peer = true;
        }
    }

    (
        to_node.is_prod,
        to_node.is_dev,
        to_node.is_optional,
        to_node.is_peer,
    ) != before
}

/// Assign dependency types (prod/dev/optional/peer) across the whole graph.
///
/// This is the single source of truth for node types: the BFS only builds graph
/// structure and never touches types, so this pass — run once after the tree is
/// complete — cannot observe a half-built tree and therefore can't leave stale
/// `dev` marks on the subtree of a node that turns out to be prod (the bug that
/// motivated centralizing it here).
///
/// It applies the per-edge propagation rules ([`update_node_type_from_edge`])
/// over every resolved edge, starting from the importer roots (whose
/// `is_root`/edge type seeds their direct deps) and re-queuing a node's children
/// whenever its flags change, until nothing changes.
///
/// Termination relies on the type lattice being monotone: each node's flags only
/// move "up" (dev/optional/peer may be set once, then `is_prod` is absorbing —
/// once set it clears the others and the `!is_prod` guards freeze the node), so
/// every node changes O(1) times.
pub fn compute_node_types(graph: &mut DependencyGraph) {
    // Seed only the importers (root + workspace members); their edge types seed
    // their direct deps, and the fixpoint re-queues every changed node's
    // children, so the whole reachable graph is covered without visiting leaves
    // that have nothing to propagate.
    let mut queued: HashSet<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|&i| {
            graph
                .get_node(i)
                .is_some_and(|n| n.is_root() || n.is_workspace())
        })
        .collect();
    let mut worklist: VecDeque<NodeIndex> = queued.iter().copied().collect();

    while let Some(src) = worklist.pop_front() {
        queued.remove(&src);

        // Snapshot resolved out-edges so we can mutate targets while iterating.
        let edges: Vec<(NodeIndex, EdgeType)> = graph
            .get_dependency_edges(src)
            .into_iter()
            .filter_map(|(_, edge)| {
                edge.to
                    .filter(|_| edge.valid)
                    .map(|to| (to, edge.edge_type))
            })
            .collect();

        for (to, edge_type) in edges {
            if update_node_type_from_edge(graph, src, to, &edge_type) && queued.insert(to) {
                worklist.push_back(to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::model::graph::PackageNode;
    use crate::model::manifest::CoreVersionManifest;
    use crate::model::package_json::PackageJson;

    fn create_version_manifest(name: &str, version: &str) -> CoreVersionManifest {
        CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        }
    }

    // Helper to create a graph with source -> target for testing update_node_type_from_edge
    // Returns (graph, source_index, target_index) where source is NOT root
    fn create_source_target_graph() -> (DependencyGraph, NodeIndex, NodeIndex) {
        let root_pkg = PackageJson::new("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), root_pkg);

        // Add source node (non-root)
        let source = PackageNode::from_version_manifest(
            "source".to_string(),
            PathBuf::from("node_modules/source"),
            Arc::new(create_version_manifest("source", "1.0.0")),
        );
        let source_index = graph.add_node(source);

        // Add target node
        let target = PackageNode::from_version_manifest(
            "target".to_string(),
            PathBuf::from("node_modules/target"),
            Arc::new(create_version_manifest("target", "1.0.0")),
        );
        let target_index = graph.add_node(target);

        (graph, source_index, target_index)
    }

    #[test]
    fn test_update_node_type_prod_optional_edge() {
        // prod source with optional edge -> target is optional only
        let (mut graph, source_index, target_index) = create_source_target_graph();

        // Mark source as prod
        graph.get_node_mut(source_index).unwrap().is_prod = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Optional);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(!target.is_dev, "should not be dev");
        assert!(target.is_optional, "should be optional");
    }

    #[test]
    fn test_update_node_type_dev_optional_edge() {
        // dev source with optional edge -> target is dev + optional
        let (mut graph, source_index, target_index) = create_source_target_graph();

        // Mark source as dev
        graph.get_node_mut(source_index).unwrap().is_dev = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Optional);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(target.is_dev, "should be dev");
        assert!(target.is_optional, "should be optional");
    }

    #[test]
    fn test_update_node_type_prod_prod_edge() {
        // prod source with prod edge -> target is prod
        let (mut graph, source_index, target_index) = create_source_target_graph();

        graph.get_node_mut(source_index).unwrap().is_prod = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Prod);

        let target = graph.get_node(target_index).unwrap();
        assert!(target.is_prod, "should be prod");
        assert!(!target.is_dev, "should not be dev");
        assert!(!target.is_optional, "should not be optional");
    }

    #[test]
    fn test_update_node_type_dev_prod_edge() {
        // dev source with prod edge -> target is dev only
        let (mut graph, source_index, target_index) = create_source_target_graph();

        graph.get_node_mut(source_index).unwrap().is_dev = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Prod);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(target.is_dev, "should be dev");
        assert!(!target.is_optional, "should not be optional");
    }

    #[test]
    fn test_update_node_type_dev_optional_source_propagates_both() {
        // dev+optional source (devOptional) with prod edge -> target is dev + optional
        let (mut graph, source_index, target_index) = create_source_target_graph();

        // Source is both dev and optional (devOptional package)
        graph.get_node_mut(source_index).unwrap().is_dev = true;
        graph.get_node_mut(source_index).unwrap().is_optional = true;

        update_node_type_from_edge(&mut graph, source_index, target_index, &EdgeType::Prod);

        let target = graph.get_node(target_index).unwrap();
        assert!(!target.is_prod, "should not be prod");
        assert!(
            target.is_dev,
            "should be dev (inherited from devOptional source)"
        );
        assert!(
            target.is_optional,
            "should be optional (inherited from devOptional source)"
        );
    }
}
