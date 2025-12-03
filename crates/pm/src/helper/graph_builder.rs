use anyhow::Result;
use petgraph::graph::{EdgeIndex, NodeIndex};

use crate::model::graph::{DependencyGraph, FindResult, PackageNode};
use crate::model::node::EdgeType;
use crate::util::config::get_legacy_peer_deps;
use crate::util::logger::{PROGRESS_BAR, log_progress};
use crate::util::registry::{ResolvedPackage, resolve_dependency};

/// Represents UNRESOLVED dependency edge information extracted from the graph
/// Only used for edges that need to be resolved (is_valid = false)
/// This structure improves readability compared to using tuples
#[derive(Debug, Clone)]
struct DependencyEdgeInfo {
    edge_id: EdgeIndex,
    name: String,
    spec: String,
    edge_type: EdgeType,
}

/// Snapshot of node dependency flags to avoid borrowing conflicts
/// Used when we need to read from one node and write to another
#[derive(Debug, Clone, Copy)]
struct NodeFlags {
    is_root: bool,
    is_prod: bool,
    is_dev: bool,
    is_optional: bool,
    is_peer: bool,
}

/// Collect ONLY unresolved dependency edges from a node
/// This optimization avoids cloning name/spec for already-resolved edges (workspace edges, etc.)
/// This helper function improves code readability by avoiding complex tuple destructuring
fn collect_dependency_edges(
    graph: &DependencyGraph,
    node_index: NodeIndex,
) -> Vec<DependencyEdgeInfo> {
    let dep_edges = graph.get_dependency_edges(node_index);
    dep_edges
        .into_iter()
        .filter(|(_, dep)| !dep.valid) // Only collect unresolved edges
        .map(|(edge_id, dep)| DependencyEdgeInfo {
            edge_id,
            name: dep.name.clone(),
            spec: dep.spec.clone(),
            edge_type: dep.edge_type, // EdgeType is Copy, no need to clone
        })
        .collect()
}

/// Build dependency tree using single-threaded BFS traversal
pub async fn build_deps(graph: &mut DependencyGraph) -> Result<()> {
    let legacy_peer_deps = get_legacy_peer_deps().await;
    tracing::debug!("going to build deps for root, legacy_peer_deps: {legacy_peer_deps}");

    let mut current_level = vec![graph.root_index];

    while !current_level.is_empty() {
        let level_count = current_level.len();
        tracing::debug!("Starting new dependency level with {level_count} nodes");

        let mut next_level = Vec::new();

        for node_index in current_level {
            // Handle resolved edges first (no need to clone name/spec)
            let resolved_edges: Vec<_> = graph
                .get_dependency_edges(node_index)
                .into_iter()
                .filter_map(|(_, dep)| if dep.valid { dep.to } else { None })
                .collect();

            for target_idx in resolved_edges {
                if let Some(target_node) = graph.get_node(target_idx) {
                    // Only add workspace nodes to next level when from root
                    if target_node.is_workspace() && node_index == graph.root_index {
                        next_level.push(target_idx);
                    }
                }
            }

            // Collect only unresolved edges (these need name/spec cloned)
            let unresolved_edges = collect_dependency_edges(graph, node_index);

            PROGRESS_BAR.inc_length(unresolved_edges.len() as u64);

            let node_name = graph
                .get_node(node_index)
                .map(|n| n.name.as_str())
                .unwrap_or("unknown");
            log_progress(&format!("resolving {}", node_name));

            for edge_info in unresolved_edges {
                tracing::debug!(
                    "going to build deps {}@{} from [{:?}]",
                    edge_info.name,
                    edge_info.spec,
                    node_index
                );

                let start_time = std::time::Instant::now();

                // Find installation location
                match graph.find_compatible_node(node_index, &edge_info.name, &edge_info.spec) {
                    FindResult::Reuse(existing_index) => {
                        let version = graph
                            .get_node(existing_index)
                            .map(|n| n.version.as_str())
                            .unwrap_or("unknown");
                        tracing::debug!(
                            "resolved deps {}@{} => {} (reuse) took {:?}",
                            edge_info.name,
                            edge_info.spec,
                            version,
                            start_time.elapsed()
                        );

                        // Mark edge as resolved
                        graph.mark_dependency_resolved(edge_info.edge_id, existing_index);

                        // Update target node type based on edge type
                        update_node_type_from_edge(
                            graph,
                            node_index,
                            existing_index,
                            &edge_info.edge_type,
                        );
                    }
                    FindResult::Conflict(conflict_parent) | FindResult::New(conflict_parent) => {
                        tracing::debug!(
                            "Conflict/New found for {}@{}, resolving...",
                            edge_info.name,
                            edge_info.spec
                        );

                        // Check if there's an override for this dependency
                        let override_spec =
                            graph.check_override(node_index, &edge_info.name, &edge_info.spec);
                        let effective_spec: &str =
                            override_spec.as_deref().unwrap_or(&edge_info.spec);

                        // Resolve dependency from registry with effective spec
                        let resolved = match resolve_dependency(
                            &edge_info.name,
                            effective_spec,
                            &edge_info.edge_type,
                        )
                        .await?
                        {
                            Some(resolved) => {
                                tracing::debug!(
                                    "Resolved dependency {}@{} => {}",
                                    edge_info.name,
                                    edge_info.spec,
                                    resolved.version
                                );
                                resolved
                            }
                            None => {
                                tracing::debug!(
                                    "No resolution found for {}@{}",
                                    edge_info.name,
                                    edge_info.spec
                                );
                                PROGRESS_BAR.inc(1);
                                continue;
                            }
                        };

                        PROGRESS_BAR.inc(1);
                        tracing::debug!(
                            "resolved deps {}@{} => {} (conflict/new) took {:?}",
                            edge_info.name,
                            edge_info.spec,
                            resolved.version,
                            start_time.elapsed()
                        );

                        // Create new node
                        let new_node =
                            place_deps(&edge_info.name, &resolved, conflict_parent, graph);
                        let new_index = graph.add_node(new_node);

                        // Add physical edge
                        graph.add_physical_edge(conflict_parent, new_index);

                        // Mark edge as resolved
                        graph.mark_dependency_resolved(edge_info.edge_id, new_index);

                        // Update target node type based on edge type
                        update_node_type_from_edge(
                            graph,
                            node_index,
                            new_index,
                            &edge_info.edge_type,
                        );

                        // Add dependencies of the new node
                        add_dependency_edges(
                            graph,
                            new_index,
                            &resolved.manifest,
                            legacy_peer_deps,
                        )
                        .await;

                        // Add to next level for processing
                        next_level.push(new_index);
                    }
                }
            }
        }

        tracing::debug!("Level completed, next level has {} nodes", next_level.len());
        current_level = next_level;
    }

    Ok(())
}

/// Create a new package node
/// Parent node must exist in the graph (ensured by caller)
fn place_deps(
    name: &str,
    pkg: &ResolvedPackage,
    parent: NodeIndex,
    graph: &DependencyGraph,
) -> PackageNode {
    let parent_node = graph
        .get_node(parent)
        .expect("Parent node must exist in graph");

    let path = if parent_node.path.to_string_lossy().is_empty()
        || parent_node.path == std::path::Path::new(".")
    {
        std::path::PathBuf::from(format!("node_modules/{name}"))
    } else {
        parent_node.path.join(format!("node_modules/{name}"))
    };

    let new_node = PackageNode::new(name.to_string(), path, pkg.manifest.clone());

    tracing::debug!(
        "\nInstalling {}@{} under parent {:?}",
        new_node.name,
        new_node.version,
        parent
    );

    new_node
}

/// Add dependency edges for a node
/// Node must exist in the graph (ensured by caller)
async fn add_dependency_edges(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    manifest: &serde_json::Value,
    legacy_peer_deps: bool,
) {
    // Extract node name once for logging
    let node_name = graph
        .get_node(node_index)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let dep_types = if legacy_peer_deps {
        vec![
            ("dependencies", EdgeType::Prod),
            ("optionalDependencies", EdgeType::Optional),
        ]
    } else {
        vec![
            ("dependencies", EdgeType::Prod),
            ("peerDependencies", EdgeType::Peer),
            ("optionalDependencies", EdgeType::Optional),
        ]
    };

    for (field, edge_type) in dep_types {
        if let Some(deps) = manifest.get(field).and_then(|v| v.as_object()) {
            tracing::debug!("Processing {} dependencies for {}", field, node_name);
            for (name, version) in deps {
                let version_spec = version.as_str().unwrap_or("").to_string();
                graph.add_dependency_edge(node_index, name.clone(), version_spec, edge_type);
                tracing::debug!("add edge {}@{} for {}", name, version, node_name);
            }
            tracing::debug!(
                "Finished processing {} dependencies for {}",
                field,
                node_name
            );
        }
    }
}

/// Update target node type based on source node and edge type
///
/// This function propagates dependency types through the graph according to npm rules:
/// - Root dependencies directly set the target node type
/// - Prod dependencies propagate through prod edges
/// - Dev/Optional flags propagate only when appropriate
///
/// # Panics
/// Panics if from_index or to_index don't exist in the graph (should never happen in normal flow)
fn update_node_type_from_edge(
    graph: &mut DependencyGraph,
    from_index: NodeIndex,
    to_index: NodeIndex,
    edge_type: &EdgeType,
) {
    // Extract source node information to avoid borrowing conflicts
    // We need these flags to determine how to mark the target node
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

    // Update target node based on source node type and edge type
    let to_node = graph
        .get_node_mut(to_index)
        .expect("Target node must exist in graph");

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
        // If source is prod, and edge is prod, target must be prod
        if source_flags.is_prod && *edge_type == EdgeType::Prod {
            to_node.is_prod = true;
            to_node.is_dev = false;
            to_node.is_optional = false;
            to_node.is_peer = false;
        }
        // Propagate dev
        else if source_flags.is_dev && *edge_type == EdgeType::Dev && !to_node.is_prod {
            to_node.is_dev = true;
        }
        // Propagate optional
        else if source_flags.is_optional
            && *edge_type == EdgeType::Optional
            && !to_node.is_prod
            && !to_node.is_dev
        {
            to_node.is_optional = true;
        }
        // Propagate peer
        else if source_flags.is_peer
            && *edge_type == EdgeType::Peer
            && !to_node.is_prod
            && !to_node.is_dev
        {
            to_node.is_peer = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_place_deps() {
        let pkg = json!({"name": "root", "version": "1.0.0"});
        let graph = DependencyGraph::new(PathBuf::from("."), pkg);

        let resolved = ResolvedPackage {
            name: "lodash".to_string(),
            version: "4.17.21".to_string(),
            manifest: json!({"name": "lodash", "version": "4.17.21"}),
        };

        let new_node = place_deps("lodash", &resolved, graph.root_index, &graph);

        assert_eq!(new_node.name, "lodash");
        assert_eq!(new_node.version, "4.17.21");
        assert_eq!(new_node.path, PathBuf::from("node_modules/lodash"));
    }
}
