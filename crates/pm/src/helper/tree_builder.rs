//! TreeBuilder - builds workspace dependency graph.
//!
//! This module is only used for workspace topology analysis.
//! For full dependency resolution, use ruborist's `build_deps` API directly.

use std::path::PathBuf;

use anyhow::Result;
use utoo_ruborist::builder::{
    DevDeps, EdgeContext, add_edges_from, add_workspace_member, resolve_workspace_member_edges,
};
use utoo_ruborist::graph::{DependencyGraph, EdgeType};
use utoo_ruborist::runtime::install_runtime_from_map;

use crate::helper::ruborist_context::Context as FsContext;
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
        let deps = match root_node.manifest.engines() {
            Some(engines) => install_runtime_from_map(engines),
            None => return Ok(()),
        };
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
        let workspaces = FsContext::discovery()
            .find_workspaces(&self.path)
            .await
            .map_err(|e| {
                let err_msg = e
                    .chain()
                    .map(|err| format!("  {err}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                anyhow::anyhow!(err_msg)
            })?;

        let peer_deps = get_peer_deps().await;
        let edge_ctx = EdgeContext::new(peer_deps, DevDeps::Include);
        let root_index = graph.root_index;

        for ws in workspaces {
            tracing::debug!("Added workspace: {} {:?}", ws.name, ws.path);
            add_workspace_member(
                graph,
                root_index,
                &ws.name,
                ws.path,
                &ws.package_json,
                &edge_ctx,
            );
        }
        // Settle importer-declared workspace: edges so topology consumers see
        // resolved member edges, matching the install graph's init shape.
        resolve_workspace_member_edges(graph);

        Ok(())
    }

    /// Build workspace tree (only resolves workspace dependencies, not external packages).
    ///
    /// This is used for workspace topology analysis, not full dependency resolution.
    pub async fn build_workspace_tree(&mut self) -> Result<()> {
        let mut graph = self.init_tree().await?;

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

        self.ideal_tree = Some(graph);
        Ok(())
    }
}
