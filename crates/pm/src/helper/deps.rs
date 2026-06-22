use petgraph::Graph;
use petgraph::algo::{kosaraju_scc, toposort};
use petgraph::prelude::*;
use std::collections::HashMap;

use anyhow::{Result, anyhow};

/// Build a directed graph from node names and `(from, to)` edge pairs.
fn build_graph(node_list: &[String], edges: &[(&str, &str)]) -> Graph<String, ()> {
    let mut graph = Graph::<String, ()>::new();
    let mut indices = HashMap::<&str, NodeIndex>::new();

    for name in node_list {
        let idx = graph.add_node(name.clone());
        indices.insert(name, idx);
    }

    for &(from, to) in edges {
        if let (Some(&a), Some(&b)) = (indices.get(from), indices.get(to)) {
            graph.add_edge(a, b, ());
        }
    }

    graph
}

/// Return the cycle groups of an already-built graph.
///
/// kosaraju_scc returns all strongly connected components (SCCs) — maximal
/// sets of nodes where every node is reachable from every other. An SCC
/// with >1 node (or a single self-loop) is a cycle.
fn cycle_groups(graph: &Graph<String, ()>) -> Vec<Vec<String>> {
    kosaraju_scc(graph)
        .into_iter()
        .filter(|scc| {
            scc.len() > 1 || (scc.len() == 1 && graph.find_edge(scc[0], scc[0]).is_some())
        })
        .map(|scc| {
            scc.into_iter()
                .filter_map(|idx| graph.node_weight(idx).cloned())
                .collect()
        })
        .collect()
}

/// Return groups of node names that form cycles.
///
/// Each returned `Vec<String>` is one cycle group. Returns empty when acyclic.
pub fn find_cycle_groups(node_list: &[String], edges: &[(&str, &str)]) -> Vec<Vec<String>> {
    cycle_groups(&build_graph(node_list, edges))
}

/// Compute topological ordering of nodes based on dependency edges.
///
/// Returns layers where each layer contains nodes that can be processed in parallel.
/// `edges` are `(from, to)` pairs meaning "to depends on from".
pub fn compute_topological_layers(
    node_list: &[String],
    edges: &[(&str, &str)],
) -> Result<Vec<Vec<String>>> {
    let graph = build_graph(node_list, edges);
    let cycles = cycle_groups(&graph);

    if !cycles.is_empty() {
        let msgs: Vec<_> = cycles
            .into_iter()
            .map(|mut c| {
                c.sort();
                let first = c[0].clone();
                c.push(first);
                format!("Cycle detected: {}", c.join(" <- "))
            })
            .collect();
        return Err(anyhow!(msgs.join("; ")));
    }

    let topo_sort =
        toposort(&graph, None).map_err(|e| anyhow!("Topological sort failed: {e:?}"))?;

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

    for &node_idx in topo_order {
        let layer = graph
            .edges_directed(node_idx, petgraph::Incoming)
            .map(|edge| {
                let dep_node = edge.source();
                node_to_layer.get(&dep_node).copied().unwrap_or(0) + 1
            })
            .max()
            .unwrap_or(0);

        node_to_layer.insert(node_idx, layer);

        while layers.len() <= layer {
            layers.push(Vec::new());
        }

        if let Some(node_name) = graph.node_weight(node_idx) {
            layers[layer].push(node_name.clone());
        }
    }

    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_compute_topological_layers_simple() {
        let n = nodes(&["A", "B"]);
        let edges = vec![("A", "B")];

        let result = compute_topological_layers(&n, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"]]);
    }

    #[test]
    fn test_compute_topological_layers_chain() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C")];

        let result = compute_topological_layers(&n, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_compute_topological_layers_parallel() {
        let n = nodes(&["A", "B", "C", "D"]);
        let edges = vec![("A", "B"), ("C", "D")];

        let result = compute_topological_layers(&n, &edges).unwrap();
        assert_eq!(result.len(), 2);

        let mut first_layer = result[0].clone();
        first_layer.sort();
        assert_eq!(first_layer, vec!["A", "C"]);

        let mut second_layer = result[1].clone();
        second_layer.sort();
        assert_eq!(second_layer, vec!["B", "D"]);
    }

    #[test]
    fn test_no_cycle() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C")];

        let result = compute_topological_layers(&n, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_with_cycle() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C"), ("C", "A")];

        let result = compute_topological_layers(&n, &edges);
        assert!(result.is_err());
    }

    #[test]
    fn test_complex_dependency_graph() {
        let n = nodes(&["A", "B", "C", "D", "E", "F"]);
        let edges = vec![
            ("A", "B"),
            ("A", "C"),
            ("B", "D"),
            ("C", "D"),
            ("D", "E"),
            ("C", "F"),
        ];

        let result = compute_topological_layers(&n, &edges).unwrap();

        assert_eq!(result[0], vec!["A"]);

        let mut layer1 = result[1].clone();
        layer1.sort();
        assert_eq!(layer1, vec!["B", "C"]);

        let mut layer2 = result[2].clone();
        layer2.sort();
        assert_eq!(layer2, vec!["D", "F"]);

        assert_eq!(result[3], vec!["E"]);
    }

    #[test]
    fn test_empty_graph() {
        let result = compute_topological_layers(&[], &[]).unwrap();
        assert_eq!(result, Vec::<Vec<String>>::new());
    }

    #[test]
    fn test_self_dependency() {
        let n = nodes(&["A"]);
        let edges = vec![("A", "A")];

        let result = compute_topological_layers(&n, &edges);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_edges() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("A", "B"), ("B", "C")];

        let result = compute_topological_layers(&n, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_with_isolated_nodes() {
        let n = nodes(&["A", "B", "C", "D"]);
        let edges = vec![("A", "B"), ("A", "C")];

        let result = compute_topological_layers(&n, &edges).unwrap();

        let mut layer0 = result[0].clone();
        layer0.sort();
        assert_eq!(layer0, vec!["A", "D"]);

        let mut layer1 = result[1].clone();
        layer1.sort();
        assert_eq!(layer1, vec!["B", "C"]);
    }

    #[test]
    fn test_with_node_objects() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C")];

        let result = compute_topological_layers(&n, &edges).unwrap();
        assert_eq!(result, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn test_cycle_error_message_contains_cycle_nodes() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C"), ("C", "A")];

        let result = compute_topological_layers(&n, &edges);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert_eq!(err_msg, "Cycle detected: A <- B <- C <- A");
    }

    #[test]
    fn test_multiple_cycles() {
        let n = nodes(&["A", "B", "C", "D", "E", "F"]);
        let edges = vec![
            ("A", "B"),
            ("B", "C"),
            ("C", "A"),
            ("D", "E"),
            ("E", "D"),
            ("F", "C"),
        ];

        let result = compute_topological_layers(&n, &edges);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert_eq!(
            err_msg,
            "Cycle detected: D <- E <- D; Cycle detected: A <- B <- C <- A"
        );
    }

    #[test]
    fn test_find_cycles_no_cycle() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C")];
        assert!(find_cycle_groups(&n, &edges).is_empty());
    }

    #[test]
    fn test_find_cycles_one_cycle() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![("A", "B"), ("B", "C"), ("C", "A")];
        let groups = find_cycle_groups(&n, &edges);
        assert_eq!(groups.len(), 1);
        let mut g = groups[0].clone();
        g.sort();
        assert_eq!(g, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_find_cycles_two_cycles() {
        let n = nodes(&["A", "B", "C", "D", "E"]);
        let edges = vec![("A", "B"), ("B", "A"), ("C", "D"), ("D", "E"), ("E", "C")];
        let mut groups = find_cycle_groups(&n, &edges);
        assert_eq!(groups.len(), 2);
        for g in &mut groups {
            g.sort();
        }
        groups.sort();
        assert_eq!(groups, vec![vec!["A", "B"], vec!["C", "D", "E"]]);
    }
}
