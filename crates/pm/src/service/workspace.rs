use crate::helper::deps::{compute_topological_layers, find_cycle_groups};
use crate::helper::tree_builder::TreeBuilder;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use utoo_ruborist::graph::EdgeType;

/// Workspace dependency edge
#[derive(Debug, Clone)]
pub struct WorkspaceEdge {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
}

/// Workspace node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceNode {
    pub name: String,
    pub path: PathBuf,
}

/// Workspace topology information
#[derive(Debug, Clone)]
pub struct WorkspaceTopology {
    pub edges: Vec<WorkspaceEdge>,
    pub topology: Vec<Vec<String>>,
    pub nodes: Vec<WorkspaceNode>,
}

/// Service for workspace operations
pub struct WorkspaceService;

impl WorkspaceService {
    /// Build workspace topology by analyzing dependencies between workspaces
    pub async fn build_workspace_topology(cwd: &Path) -> Result<WorkspaceTopology> {
        let mut builder = TreeBuilder::new(cwd);
        builder.build_workspace_tree().await?;

        let Some(graph) = &builder.ideal_tree else {
            return Ok(WorkspaceTopology {
                edges: Vec::new(),
                topology: Vec::new(),
                nodes: Vec::new(),
            });
        };

        let mut node_list = Vec::new();
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut workspace_names = HashSet::new();

        // Get all workspace nodes (excluding links)
        let workspace_nodes = graph.get_workspace_nodes();

        for node_idx in &workspace_nodes {
            let node = graph
                .get_node(*node_idx)
                .expect("workspace node index must be valid");
            let name = node.name.clone();

            workspace_names.insert(name.clone());
            node_list.push(name.clone());
            nodes.push(WorkspaceNode {
                name,
                path: node
                    .path
                    .strip_prefix(cwd)
                    .unwrap_or(&node.path)
                    .to_path_buf(),
            });
        }

        // Collect dependency edges between workspaces
        for (i, node_idx) in workspace_nodes.iter().enumerate() {
            let node_name = &nodes[i].name;

            for (_, dep) in graph.get_dependency_edges(*node_idx) {
                if !dep.valid {
                    continue;
                }
                let Some(target_idx) = dep.to else {
                    continue;
                };
                let target_node = graph
                    .get_node(target_idx)
                    .expect("target node index must be valid");
                if workspace_names.contains(&target_node.name) {
                    edges.push(WorkspaceEdge {
                        from: target_node.name.clone(),
                        to: node_name.clone(),
                        edge_type: dep.edge_type,
                    });
                }
            }
        }

        // Detect cycles once; if any exist, drop dev edges inside the
        // same SCC before running the topological sort.
        let all_pairs: Vec<_> = edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        let cycles = find_cycle_groups(&node_list, &all_pairs);
        let topology = if cycles.is_empty() {
            compute_topological_layers(&node_list, &all_pairs)?
        } else {
            let mut node_to_group = HashMap::new();
            for (i, group) in cycles.iter().enumerate() {
                for name in group {
                    node_to_group.insert(name.as_str(), i);
                }
            }
            let filtered: Vec<_> = edges
                .iter()
                .filter(|e| {
                    !(e.edge_type == EdgeType::Dev
                        && node_to_group.contains_key(e.from.as_str())
                        && node_to_group.get(e.from.as_str()) == node_to_group.get(e.to.as_str()))
                })
                .map(|e| (e.from.as_str(), e.to.as_str()))
                .collect();
            compute_topological_layers(&node_list, &filtered)?
        };

        Ok(WorkspaceTopology {
            edges,
            topology,
            nodes,
        })
    }

    /// Get only the topological ordering of workspaces
    pub async fn get_workspace_topology(cwd: &Path) -> Result<Vec<Vec<String>>> {
        let topology = Self::build_workspace_topology(cwd).await?;
        Ok(topology.topology)
    }

    /// Build workspace topology and generate JSON output for file writing
    pub async fn build_workspace_json(cwd: &Path) -> Result<serde_json::Value> {
        let topology = Self::build_workspace_topology(cwd).await?;

        let edges_json: Vec<serde_json::Value> = topology
            .edges
            .iter()
            .map(|edge| json!([edge.from.clone(), edge.to.clone()]))
            .collect();

        Ok(json!({
            "nodeList": topology.nodes,
            "edges": edges_json,
            "topology": topology.topology,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    /// Create a two-workspace (A, B) mock structure with custom package.json content.
    fn create_mock_workspace_with(dir: &Path, a_json: &str, b_json: &str) {
        let a_dir = dir.join("A");
        let b_dir = dir.join("B");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{
                "name": "root",
                "private": true,
                "workspaces": ["A", "B"]
            }"#,
        )
        .unwrap();
        fs::write(a_dir.join("package.json"), a_json).unwrap();
        fs::write(b_dir.join("package.json"), b_json).unwrap();
    }

    fn create_mock_workspace(dir: &Path) {
        create_mock_workspace_with(
            dir,
            r#"{"name":"A"}"#,
            r#"{"name":"B","dependencies":{"A":"*"}}"#,
        );
    }

    #[tokio::test]
    async fn test_dev_deps_cycle_falls_back() {
        // A devDepends on B, B devDepends on A — cycle via devDeps only.
        // Should fall back to omitting dev edges instead of erroring.
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"A","devDependencies":{"B":"*"}}"#,
            r#"{"name":"B","devDependencies":{"A":"*"}}"#,
        );
        let result = WorkspaceService::build_workspace_topology(temp.path()).await;
        assert!(result.is_ok(), "devDeps cycle should fall back, not error");
        let topo = result.unwrap();
        assert_eq!(topo.edges.len(), 2);
        // Fallback: no prod edges → no ordering constraints → single layer
        assert_eq!(topo.topology.len(), 1);
        assert_eq!(topo.topology[0].len(), 2);
    }

    #[tokio::test]
    async fn test_dev_deps_affect_ordering_when_no_cycle() {
        // A devDepends on B (no cycle) — devDep should still affect ordering
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"A","devDependencies":{"B":"*"}}"#,
            r#"{"name":"B"}"#,
        );
        let result = WorkspaceService::build_workspace_topology(temp.path()).await;
        assert!(result.is_ok());
        let topo = result.unwrap();
        // B first, then A (A devDepends on B → B should be built first)
        assert_eq!(topo.topology, vec![vec!["B"], vec!["A"]]);
    }

    #[tokio::test]
    async fn test_mixed_prod_and_dev_deps_cycle_falls_back() {
        // A prod→B, B dev→A — forms a cycle with all edges,
        // falls back to prod-only where only A→B remains
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"A","dependencies":{"B":"*"}}"#,
            r#"{"name":"B","devDependencies":{"A":"*"}}"#,
        );
        let result = WorkspaceService::build_workspace_topology(temp.path()).await;
        assert!(result.is_ok(), "mixed prod+dev cycle should fall back");
        let topo = result.unwrap();
        assert_eq!(topo.edges.len(), 2);
        // Fallback ordering: only prod edge (A depends on B) → B first
        assert_eq!(topo.topology.len(), 2);
    }

    #[tokio::test]
    async fn test_build_workspace_json_edges_order() {
        let temp = tempdir().unwrap();
        create_mock_workspace(temp.path());
        let result = WorkspaceService::build_workspace_json(temp.path()).await;
        println!("{result:?}");
        assert!(result.is_ok(), "build_workspace_json should succeed");
        let json = result.unwrap();
        let edges = json.get("edges").unwrap().as_array().unwrap();
        // Edges should be [["A", "B"]], meaning B depends on A
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0], json!(["A", "B"]));
    }
}
