use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::helper::graph_builder::build_deps;
use crate::helper::install_runtime::install_runtime;
use crate::helper::workspace::find_workspaces;
use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::node::EdgeType;
use crate::util::config::get_legacy_peer_deps;
use crate::util::json::load_package_json_from_path;
use crate::util::logger::{finish_progress_bar, start_progress_bar};
use crate::util::registry::{load_cache, store_cache};

/// Ruborist - manages dependency graph building
pub struct Ruborist {
    path: PathBuf,
    pub ideal_tree: Option<DependencyGraph>,
}

impl Ruborist {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            ideal_tree: None,
        }
    }

    async fn init_runtime(&self, graph: &mut DependencyGraph) -> Result<()> {
        let root_node = graph.get_node(graph.root_index).unwrap();
        let deps = install_runtime(root_node.package.get("engines").unwrap_or(&Value::Null))?;
        for (name, version) in deps {
            graph.add_dependency_edge(graph.root_index, name, version, EdgeType::Optional);
        }
        Ok(())
    }

    async fn init_tree(&self) -> Result<DependencyGraph> {
        // Load package.json
        let pkg = load_package_json_from_path(&self.path).await?;

        // Create dependency graph with root node
        let mut graph = DependencyGraph::new(self.path.clone(), pkg.clone());
        tracing::debug!("root node created at {:?}", graph.root_index);

        // Initialize runtime dependencies
        self.init_runtime(&mut graph).await?;

        // Initialize workspaces
        self.init_workspaces(&mut graph).await?;

        // Collect dependency types
        let legacy_peer_deps = get_legacy_peer_deps().await;
        let dep_types = if legacy_peer_deps {
            vec![
                ("dependencies", EdgeType::Prod),
                ("devDependencies", EdgeType::Dev),
                ("optionalDependencies", EdgeType::Optional),
            ]
        } else {
            vec![
                ("dependencies", EdgeType::Prod),
                ("devDependencies", EdgeType::Dev),
                ("peerDependencies", EdgeType::Peer),
                ("optionalDependencies", EdgeType::Optional),
            ]
        };

        // Add root dependencies
        for (field, dep_type) in dep_types {
            if let Some(deps) = pkg.get(field).and_then(|v| v.as_object()) {
                for (name, version) in deps {
                    tracing::debug!("{name}: {version}");
                    let version_spec = version.as_str().unwrap_or("").to_string();
                    graph.add_dependency_edge(
                        graph.root_index,
                        name.clone(),
                        version_spec,
                        dep_type,
                    );
                    tracing::debug!("add edge {}@{}", name, version);
                }
            }
        }

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

        // Process each workspace member
        for (name, path, pkg) in workspaces {
            let version = pkg["version"].as_str().unwrap_or("").to_string();

            // Create workspace node
            let workspace_node =
                PackageNode::new_workspace(name.clone(), path.clone(), pkg.clone());
            let workspace_index = graph.add_node(workspace_node);

            // Create link node
            let link_node =
                PackageNode::new_link(name.clone(), path.clone(), pkg.clone(), version.clone());
            let link_index = graph.add_node(link_node);

            // Add physical edges
            graph.add_physical_edge(graph.root_index, workspace_index);
            graph.add_physical_edge(graph.root_index, link_index);

            // Create and mark dependency edge as resolved
            let dep_edge_id = graph.add_dependency_edge(
                graph.root_index,
                name.clone(),
                version.clone(),
                EdgeType::Prod,
            );
            graph.mark_dependency_resolved(dep_edge_id, workspace_index);

            tracing::debug!("Added workspace: {} {:?}", name, path);

            // Process workspace dependencies
            let legacy_peer_deps = get_legacy_peer_deps().await;
            let dep_types = if legacy_peer_deps {
                vec![
                    ("devDependencies", EdgeType::Dev),
                    ("dependencies", EdgeType::Prod),
                    ("optionalDependencies", EdgeType::Optional),
                ]
            } else {
                vec![
                    ("devDependencies", EdgeType::Dev),
                    ("dependencies", EdgeType::Prod),
                    ("peerDependencies", EdgeType::Peer),
                    ("optionalDependencies", EdgeType::Optional),
                ]
            };

            for (field, edge_type) in dep_types {
                if let Some(deps) = pkg.get(field).and_then(|v| v.as_object()) {
                    for (dep_name, version) in deps {
                        let version_spec = version.as_str().unwrap_or("").to_string();
                        graph.add_dependency_edge(
                            workspace_index,
                            dep_name.clone(),
                            version_spec,
                            edge_type,
                        );
                        tracing::debug!("add edge {}@{} for {}", dep_name, version, name);
                    }
                }
            }
        }

        Ok(())
    }

    /// Build workspace tree (only resolves workspace dependencies, not external packages)
    pub async fn build_workspace_tree(&mut self) -> Result<()> {
        let mut graph = self.init_tree().await?;

        start_progress_bar();

        // Build a map of workspace nodes for quick lookup
        let mut workspace_map = std::collections::HashMap::new();
        for node_idx in graph.graph.node_indices() {
            let node = graph.get_node(node_idx).unwrap();
            if node.is_workspace() {
                workspace_map.insert(node.name.clone(), node_idx);
            }
        }

        // Collect all workspace dependency resolutions first
        let mut resolutions = Vec::new();

        for node_idx in graph.graph.node_indices() {
            // Check if this is a workspace node (not link)
            let (is_link, is_workspace, node_name) = {
                let node = graph.get_node(node_idx).unwrap();
                (node.is_link(), node.is_workspace(), node.name.clone())
            };

            if is_link || !is_workspace {
                continue;
            }

            // Get dependency edges for this workspace
            let dep_edges: Vec<_> = graph.get_dependency_edges(node_idx);

            for (edge_id, dep_edge) in dep_edges {
                // Check if this dependency is another workspace
                if let Some(&dep_workspace_idx) = workspace_map.get(&dep_edge.name) {
                    resolutions.push((
                        edge_id,
                        dep_workspace_idx,
                        node_name.clone(),
                        dep_edge.name.clone(),
                    ));
                }
            }
        }

        // Now apply all resolutions
        for (edge_id, dep_workspace_idx, from_name, to_name) in resolutions {
            graph.mark_dependency_resolved(edge_id, dep_workspace_idx);

            tracing::debug!("Workspace dependency: {} -> {}", from_name, to_name);
        }

        finish_progress_bar("workspace resolved");
        self.ideal_tree = Some(graph);
        Ok(())
    }

    pub async fn build_ideal_tree(&mut self) -> Result<()> {
        let cache_path = self.path.join("./node_modules/.utoo-manifest.json");
        load_cache(&cache_path)
            .await
            .context("Failed to load cache")?;

        let mut graph = self.init_tree().await?;

        // Check if project cache exists
        let project_cache_path = self.path.join("node_modules/.utoo-manifest.json");
        let has_project_cache = tokio::fs::try_exists(&project_cache_path)
            .await
            .unwrap_or(false);

        // Only preload if project cache doesn't exist
        if !has_project_cache {
            let legacy_peer_deps = get_legacy_peer_deps().await;
            let mut all_deps = std::collections::HashSet::new();

            // Collect root dependencies
            let root_node = graph.get_node(graph.root_index).unwrap();
            let root_deps = crate::service::preload::PreloadService::collect_root_dependencies(
                &root_node.package,
                legacy_peer_deps,
            );
            for dep in root_deps {
                all_deps.insert(dep);
            }

            // Collect workspace dependencies
            for node_index in graph.graph.node_indices() {
                let node = graph.get_node(node_index).unwrap();
                if node.is_workspace() {
                    let workspace_deps =
                        crate::service::preload::PreloadService::collect_root_dependencies(
                            &node.package,
                            legacy_peer_deps,
                        );
                    for dep in workspace_deps {
                        all_deps.insert(dep);
                    }
                }
            }

            let initial_deps: Vec<_> = all_deps.into_iter().collect();

            tracing::debug!(
                "No project cache found, preloading {} dependencies (root + workspaces)",
                initial_deps.len()
            );
            if let Err(e) =
                crate::service::preload::PreloadService::preload(initial_deps, true).await
            {
                tracing::warn!("Preload failed, continuing with normal resolution: {}", e);
            }
        } else {
            tracing::debug!(
                "Project cache found at {}, skipping preload",
                project_cache_path.display()
            );
        }

        start_progress_bar();
        build_deps(&mut graph).await?;

        // TODO: Implement dedup_deps for graph
        // self.dedup_deps(&mut graph).await?;

        store_cache(&cache_path)
            .await
            .context("Failed to store cache")?;
        finish_progress_bar("package-lock.json resolved");

        self.ideal_tree = Some(graph);
        Ok(())
    }
}
