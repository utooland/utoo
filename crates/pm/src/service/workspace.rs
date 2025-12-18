use crate::helper::deps::{Edge, Node, compute_topological_layers};
use crate::helper::tree_builder::TreeBuilder;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Workspace node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceNode {
    pub name: String,
    pub path: PathBuf,
}

/// Workspace topology information
#[derive(Debug, Clone)]
pub struct WorkspaceTopology {
    pub edges: Vec<Edge>,
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

        // Collect workspace nodes
        for node_idx in &workspace_nodes {
            let node = graph
                .get_node(*node_idx)
                .expect("workspace node index must be valid");
            let name = node.name.clone();

            workspace_names.insert(name.clone());
            node_list.push(Node::new(name.clone()));
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
        for node_idx in &workspace_nodes {
            let node = graph
                .get_node(*node_idx)
                .expect("workspace node index must be valid");
            let resolved_deps = graph.get_resolved_dependencies(*node_idx);

            for (_dep_name, target_idx) in resolved_deps {
                let target_node = graph
                    .get_node(target_idx)
                    .expect("target node index must be valid");
                // Only include workspace-to-workspace dependencies
                if workspace_names.contains(&target_node.name) {
                    // Edge: [to, from] meaning "to depends on from"
                    edges.push(Edge::new(target_node.name.clone(), node.name.clone()));
                }
            }
        }

        let topology = compute_topological_layers(&node_list, &edges)?;

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

    // Helper to create a mock workspace structure with root package.json
    fn create_mock_workspace(dir: &Path) {
        // Create workspace A and B, B depends on A
        let a_dir = dir.join("A");
        let b_dir = dir.join("B");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();
        // Write root package.json with workspaces field
        fs::write(
            dir.join("package.json"),
            r#"{
                "name": "root",
                "private": true,
                "workspaces": ["A", "B"]
            }"#,
        )
        .unwrap();
        // Write package.json for A
        fs::write(a_dir.join("package.json"), r#"{"name":"A"}"#).unwrap();
        // Write package.json for B, depends on A
        fs::write(
            b_dir.join("package.json"),
            r#"{"name":"B","dependencies":{"A":"*"}}"#,
        )
        .unwrap();
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
