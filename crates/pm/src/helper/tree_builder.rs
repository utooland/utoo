//! TreeBuilder - builds workspace dependency graph.
//!
//! This module is only used for workspace topology analysis.
//! For full dependency resolution, use ruborist's `build_deps` API directly.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};
use utoo_ruborist::builder::{DevDeps, EdgeContext, add_edges_from};
use utoo_ruborist::graph::{DependencyGraph, EdgeType, PackageNode};

use crate::helper::install_runtime::install_runtime;
use crate::helper::workspace::find_workspaces;
use crate::util::logger::{finish_progress_bar, start_progress_bar};
use crate::util::user_config::{get_or_load_package_json, get_peer_deps};

/// TreeBuilder - builds workspace dependency graph.
///
/// Only used for workspace topology analysis (no network requests).
/// For full dependency resolution, use ruborist's `build_deps` API.
pub struct TreeBuilder {
    path: PathBuf,
    pub ideal_tree: Option<DependencyGraph>,
}

impl TreeBuilder {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            ideal_tree: None,
        }
    }

    async fn init_runtime(&self, graph: &mut DependencyGraph) -> Result<()> {
        let root_node = graph
            .get_node(graph.root_index)
            .expect("root node must exist");
        // Convert engines HashMap to Value for install_runtime (legacy compatibility)
        let engines_value = match root_node.manifest.engines() {
            Some(engines) => json!(engines),
            None => Value::Null,
        };
        let deps = install_runtime(&engines_value)?;
        for (name, version) in deps {
            graph.add_dependency_edge(graph.root_index, name, version, EdgeType::Optional);
        }
        Ok(())
    }

    async fn init_tree(&self) -> Result<DependencyGraph> {
        let pkg = get_or_load_package_json(&self.path).await?;

        // Create dependency graph with root node
        let mut graph = DependencyGraph::from_package_json(self.path.clone(), pkg.clone());
        tracing::debug!("root node created at {:?}", graph.root_index);

        // Initialize runtime dependencies
        self.init_runtime(&mut graph).await?;

        // Initialize workspaces
        self.init_workspaces(&mut graph).await?;

        // Add root dependencies using ruborist's shared logic
        let peer_deps = get_peer_deps().await;
        let root_index = graph.root_index;
        add_edges_from(
            &mut graph,
            root_index,
            &pkg,
            &EdgeContext::new(peer_deps, DevDeps::Include),
        );

        Ok(graph)
    }

    async fn init_workspaces(&self, graph: &mut DependencyGraph) -> Result<()> {
        let workspaces = find_workspaces(&self.path).await.map_err(|e| {
            let err_msg = e
                .chain()
                .map(|err| format!("  {err}"))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::anyhow!(err_msg)
        })?;

        let peer_deps = get_peer_deps().await;

        // Process each workspace member
        for (name, path, pkg) in workspaces {
            let version = &pkg.version;

            // Create workspace node
            let workspace_node =
                PackageNode::workspace_from_package_json(path.clone(), pkg.clone());
            let workspace_index = graph.add_node(workspace_node);

            // Create link node
            let link_node = PackageNode::link_from_package_json(path.clone(), pkg.clone());
            let link_index = graph.add_node(link_node);

            // Add physical edges
            graph.add_physical_edge(graph.root_index, workspace_index);
            graph.add_physical_edge(graph.root_index, link_index);

            // Create and mark dependency edge as resolved
            let dep_edge_id = graph.add_dependency_edge(
                graph.root_index,
                name.as_str(),
                version.as_str(),
                EdgeType::Prod,
            );
            graph.mark_dependency_resolved(dep_edge_id, workspace_index);

            tracing::debug!("Added workspace: {} {:?}", name, path);

            // Add workspace dependencies using ruborist's shared logic
            add_edges_from(
                graph,
                workspace_index,
                &pkg,
                &EdgeContext::new(peer_deps, DevDeps::Include),
            );
        }

        Ok(())
    }

    /// Build workspace tree (only resolves workspace dependencies, not external packages).
    ///
    /// This is used for workspace topology analysis, not full dependency resolution.
    pub async fn build_workspace_tree(&mut self) -> Result<()> {
        let mut graph = self.init_tree().await?;

        start_progress_bar();

        // Build a map of workspace nodes for quick lookup
        let mut workspace_map = std::collections::HashMap::new();
        for node_idx in graph.graph.node_indices() {
            let node = graph.get_node(node_idx).expect("node index must be valid");
            if node.is_workspace() {
                workspace_map.insert(node.name.clone(), node_idx);
            }
        }

        // Collect all workspace dependency resolutions first
        let mut resolutions = Vec::new();

        for node_idx in graph.graph.node_indices() {
            let node = graph.get_node(node_idx).expect("node index must be valid");
            if node.is_link() || !node.is_workspace() {
                continue;
            }

            for (edge_id, dep_edge) in graph.get_dependency_edges(node_idx) {
                if let Some(&dep_workspace_idx) = workspace_map.get(&dep_edge.name) {
                    tracing::debug!("Workspace dependency: {} -> {}", node.name, dep_edge.name);
                    resolutions.push((edge_id, dep_workspace_idx));
                }
            }
        }

        // Apply all resolutions
        for (edge_id, dep_workspace_idx) in resolutions {
            graph.mark_dependency_resolved(edge_id, dep_workspace_idx);
        }

        finish_progress_bar("workspace resolved");
        self.ideal_tree = Some(graph);
        Ok(())
    }
}
