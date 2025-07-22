use petgraph::Graph;
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::prelude::*;
use std::collections::HashMap;

use crate::util::logger::log_warning;
use anyhow::{Result, anyhow};

/// Represents a node in the dependency graph
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
}

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Represents a dependency edge in the graph
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String, // dependency (should be executed first)
    pub to: String,   // dependent (depends on from)
}

impl Edge {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// Compute topological ordering of nodes based on dependency edges using petgraph
///
/// # Parameters
/// - `node_list`: List of all nodes that should be included in the result.
///   Nodes without dependencies will be placed in the first layer.
/// - `edges`: List of dependency edges where each edge represents "to depends on from"
///
/// # Returns
/// Returns Result of layers where each layer contains nodes that can be processed in parallel
pub fn compute_topological_layers(node_list: &[Node], edges: &[Edge]) -> Result<Vec<Vec<String>>> {
    let mut graph = Graph::<String, ()>::new();
    let mut node_indices = HashMap::<String, NodeIndex>::new();

    // Add all nodes from nodeList to the graph
    for node in node_list {
        let idx = graph.add_node(node.name.clone());
        node_indices.insert(node.name.clone(), idx);
    }

    // Add edges to the graph (from -> to)
    for edge in edges {
        if let (Some(&from_idx), Some(&to_idx)) =
            (node_indices.get(&edge.from), node_indices.get(&edge.to))
        {
            // Add edge from -> to (dependency -> dependent)
            graph.add_edge(from_idx, to_idx, ());
        }
    }

    // Check for cycles first
    if is_cyclic_directed(&graph) {
        // Find all strongly connected components (SCCs)
        use petgraph::algo::kosaraju_scc;
        let sccs = kosaraju_scc(&graph);
        let mut cycles = Vec::new();
        for scc in &sccs {
            // A cycle is a SCC with more than 1 node, or a self-loop
            if scc.len() > 1 {
                // Collect node names in this cycle
                let names: Vec<_> = scc
                    .iter()
                    .filter_map(|&idx| graph.node_weight(idx))
                    .cloned()
                    .collect();
                cycles.push(names);
            } else if scc.len() == 1 {
                let idx = scc[0];
                // Check for self-loop
                if graph.find_edge(idx, idx).is_some() {
                    if let Some(name) = graph.node_weight(idx) {
                        cycles.push(vec![name.clone()]);
                    }
                }
            }
        }
        // Print all cycles
        let mut cycle_msgs = Vec::new();
        for cycle in cycles {
            let msg = format!("Cycle detected: {}", cycle.join(" <- "));
            cycle_msgs.push(msg);
        }
        return Err(anyhow!(cycle_msgs.join("; ")));
    }

    // Get topological ordering
    let topo_sort = match toposort(&graph, None) {
        Ok(sorted) => sorted,
        Err(e) => {
            log_warning("Failed to perform topological sort");
            return Err(anyhow!("Topological sort failed: {:?}", e));
        }
    };

    // Convert topological order to layers
    Ok(compute_layers_from_topo_order(&graph, &topo_sort))
}

/// Compute layers from topological ordering - simplified approach
/// Nodes in the same layer have no dependencies on each other and can be processed in parallel
fn compute_layers_from_topo_order(
    graph: &Graph<String, ()>,
    topo_order: &[NodeIndex],
) -> Vec<Vec<String>> {
    let mut layers = Vec::new();
    let mut node_to_layer = HashMap::<NodeIndex, usize>::new();

    // For each node in topological order, determine its layer
    for &node_idx in topo_order {
        // Find the maximum layer of all direct dependencies
        let layer = graph
            .edges_directed(node_idx, petgraph::Incoming)
            .map(|edge| {
                let dep_node = edge.source();
                node_to_layer.get(&dep_node).copied().unwrap_or(0) + 1
            })
            .max()
            .unwrap_or(0);

        // Assign node to the calculated layer
        node_to_layer.insert(node_idx, layer);

        // Ensure we have enough layers
        while layers.len() <= layer {
            layers.push(Vec::new());
        }

        // Add node to its layer
        if let Some(node_name) = graph.node_weight(node_idx) {
            layers[layer].push(node_name.clone());
        }
    }

    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_topological_layers_simple() {
        let node_list = vec![Node::new("A"), Node::new("B")]; // Nodes will be inferred from edges
        let edges = vec![
            Edge::new("A", "B"), // B depends on A
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"]]);
    }

    #[test]
    fn test_compute_topological_layers_chain() {
        let node_list = vec![Node::new("A"), Node::new("B"), Node::new("C")]; // Nodes will be inferred from edges
        let edges = vec![
            Edge::new("A", "B"), // B depends on A
            Edge::new("B", "C"), // C depends on B
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_compute_topological_layers_parallel() {
        let node_list = vec![
            Node::new("A"),
            Node::new("B"),
            Node::new("C"),
            Node::new("D"),
        ]; // Nodes will be inferred from edges
        let edges = vec![
            Edge::new("A", "B"), // B depends on A
            Edge::new("C", "D"), // D depends on C
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();
        assert_eq!(result.len(), 2);

        // First layer should contain A and C (in any order)
        let mut first_layer = result[0].clone();
        first_layer.sort();
        assert_eq!(first_layer, vec!["A", "C"]);

        // Second layer should contain B and D (in any order)
        let mut second_layer = result[1].clone();
        second_layer.sort();
        assert_eq!(second_layer, vec!["B", "D"]);
    }

    #[test]
    fn test_no_cycle() {
        let node_list = vec![Node::new("A"), Node::new("B"), Node::new("C")];
        let edges = vec![Edge::new("A", "B"), Edge::new("B", "C")];

        let result = compute_topological_layers(&node_list, &edges).unwrap();
        // Should successfully compute layers if no cycle
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_with_cycle() {
        let node_list = vec![Node::new("A"), Node::new("B"), Node::new("C")];
        let edges = vec![
            Edge::new("A", "B"),
            Edge::new("B", "C"),
            Edge::new("C", "A"), // Creates cycle A -> B -> C -> A
        ];

        let result = compute_topological_layers(&node_list, &edges);
        // Should return Err when cycle is detected
        assert!(result.is_err());
    }

    #[test]
    fn test_complex_dependency_graph() {
        // Test a more complex graph with multiple layers and dependencies
        let node_list = vec![
            Node::new("A"),
            Node::new("B"),
            Node::new("C"),
            Node::new("D"),
            Node::new("E"),
            Node::new("F"),
        ]; // Nodes will be inferred from edges
        let edges = vec![
            Edge::new("A", "B"), // B depends on A
            Edge::new("A", "C"), // C depends on A
            Edge::new("B", "D"), // D depends on B
            Edge::new("C", "D"), // D depends on C
            Edge::new("D", "E"), // E depends on D
            Edge::new("C", "F"), // F depends on C
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();

        // Layer 0: A (no dependencies)
        assert_eq!(result[0], vec!["A"]);

        // Layer 1: B, C (depend only on A)
        let mut layer1 = result[1].clone();
        layer1.sort();
        assert_eq!(layer1, vec!["B", "C"]);

        // Layer 2: D, F (D depends on B and C, F depends on C)
        let mut layer2 = result[2].clone();
        layer2.sort();
        assert_eq!(layer2, vec!["D", "F"]);

        // Layer 3: E (depends on D)
        assert_eq!(result[3], vec!["E"]);
    }

    #[test]
    fn test_empty_graph() {
        let node_list = vec![];
        let edges = vec![];
        let result = compute_topological_layers(&node_list, &edges).unwrap();
        assert_eq!(result, Vec::<Vec<String>>::new());
    }

    #[test]
    fn test_self_dependency() {
        // Test self-dependency (should be handled gracefully)
        let node_list = vec![Node::new("A")]; // Nodes will be inferred from edges
        let edges = vec![
            Edge::new("A", "A"), // A depends on itself
        ];

        let result = compute_topological_layers(&node_list, &edges);
        // Should detect cycle and return Err
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_edges() {
        // Test duplicate edges (should be handled correctly)
        let node_list = vec![Node::new("A"), Node::new("B"), Node::new("C")]; // Nodes will be inferred from edges
        let edges = vec![
            Edge::new("A", "B"),
            Edge::new("A", "B"), // Duplicate edge
            Edge::new("B", "C"),
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_with_isolated_nodes() {
        // Test nodes with no dependencies (isolated nodes)
        let node_list = vec![
            Node::new("A"),
            Node::new("B"),
            Node::new("C"),
            Node::new("D"), // D is isolated (no dependencies, no dependents)
        ];
        let edges = vec![
            Edge::new("A", "B"), // B depends on A
            Edge::new("A", "C"), // C depends on A
                                 // D has no edges
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();

        // Layer 0: A and D (no dependencies)
        let mut layer0 = result[0].clone();
        layer0.sort();
        assert_eq!(layer0, vec!["A", "D"]);

        // Layer 1: B and C (depend only on A)
        let mut layer1 = result[1].clone();
        layer1.sort();
        assert_eq!(layer1, vec!["B", "C"]);
    }

    #[test]
    fn test_with_node_objects() {
        // Test explicit node list with dependencies
        let node_list = vec![Node::new("A"), Node::new("B"), Node::new("C")];
        let edges = vec![
            Edge::new("A", "B"), // B depends on A
            Edge::new("B", "C"), // C depends on B
        ];

        let result = compute_topological_layers(&node_list, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_cycle_error_message_contains_cycle_nodes() {
        // Create a simple cycle: A -> B -> C -> A
        let node_list = vec![Node::new("A"), Node::new("B"), Node::new("C")];
        let edges = vec![
            Edge::new("A", "B"),
            Edge::new("B", "C"),
            Edge::new("C", "A"),
        ];

        let result = compute_topological_layers(&node_list, &edges);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        // The error message should contain all nodes in the cycle
        assert!(err_msg.contains("A"));
        assert!(err_msg.contains("B"));
        assert!(err_msg.contains("C"));
        assert!(err_msg.contains("Cycle detected"));
    }
}
