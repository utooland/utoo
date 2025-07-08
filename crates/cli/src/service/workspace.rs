use crate::helper::deps::{Edge, Node, compute_topological_layers};
use crate::helper::ruborist::Ruborist;
use crate::util::relative_path::to_relative_path;
use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;

/// Workspace topology information
#[derive(Debug, Clone)]
pub struct WorkspaceTopology {
    pub node_list: Vec<Node>,
    pub edges: Vec<Edge>,
    pub topology: Vec<Vec<String>>,
    pub node_json_list: Vec<serde_json::Value>,
}

/// Service for workspace operations
pub struct WorkspaceService;

impl WorkspaceService {
    /// Build workspace topology by analyzing dependencies between workspaces
    pub async fn build_workspace_topology(cwd: &Path) -> Result<WorkspaceTopology> {
        let mut ruborist = Ruborist::new(cwd);
        ruborist.build_workspace_tree().await?;

        if let Some(ideal_tree) = &ruborist.ideal_tree {
            let mut node_list = Vec::new();
            let mut node_json_list = Vec::new();
            let mut edges = Vec::new();
            let mut workspace_names = HashSet::new();

            // Collect workspace nodes
            for child in ideal_tree.children.read().unwrap().iter() {
                let name = child.name.clone();
                if child.is_link {
                    continue;
                }
                workspace_names.insert(name.clone());

                // Create Node struct for the helper function
                node_list.push(Node::new(name.clone()));

                // Create JSON for output file
                node_json_list.push(json!({
                    "name": name,
                    "path": to_relative_path(&child.path, cwd),
                }));
            }

            // Collect dependency edges
            for child in ideal_tree.children.read().unwrap().iter() {
                for edge in child.edges_out.read().unwrap().iter() {
                    if *edge.valid.read().unwrap()
                        && let Some(to_node) = edge.to.read().unwrap().as_ref()
                    {
                        // Create Edge struct: format is [to, from] meaning "to depends on from"
                        // So from=edge.from.name (dependency), to=to_node.name (dependent)
                        edges.push(Edge::new(edge.from.name.clone(), to_node.name.clone()));
                    }
                }
            }

            // Compute topological layers using the helper function
            let topology = compute_topological_layers(&node_list, &edges);

            Ok(WorkspaceTopology {
                node_list,
                edges,
                topology,
                node_json_list,
            })
        } else {
            // Return empty topology if no workspaces found
            Ok(WorkspaceTopology {
                node_list: Vec::new(),
                edges: Vec::new(),
                topology: Vec::new(),
                node_json_list: Vec::new(),
            })
        }
    }

    /// Get only the topological ordering of workspaces
    pub async fn get_workspace_topology(cwd: &Path) -> Result<Vec<Vec<String>>> {
        let topology = Self::build_workspace_topology(cwd).await?;
        Ok(topology.topology)
    }

    /// Build workspace topology and generate JSON output for file writing
    pub async fn build_workspace_json(cwd: &Path) -> Result<serde_json::Value> {
        let topology = Self::build_workspace_topology(cwd).await?;

        // Create edges in JSON format for output (format: [to, from] meaning "to depends on from")
        let edges_json: Vec<serde_json::Value> = topology
            .edges
            .iter()
            .map(|edge| json!([edge.from.clone(), edge.to.clone()]))
            .collect();

        Ok(json!({
            "nodeList": topology.node_json_list,
            "edges": edges_json,
            "topology": topology.topology,
        }))
    }
}
