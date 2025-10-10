use anyhow::{Context, Result};
use petgraph::Graph;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::util::logger::log_verbose;

/// Represents a package information in package-lock.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageNode {
    /// Package name
    pub name: String,
    /// Version
    pub version: String,
    /// Path in package-lock.json
    pub path: String,
    /// Dependencies list
    pub dependencies: HashMap<String, String>,
    /// Development dependencies
    pub dev_dependencies: HashMap<String, String>,
    /// Optional dependencies
    pub optional_dependencies: HashMap<String, String>,
    /// Peer dependencies
    pub peer_dependencies: HashMap<String, String>,
}

impl PackageNode {
    pub fn new(name: String, version: String, path: String) -> Self {
        Self {
            name,
            version,
            path,
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            optional_dependencies: HashMap::new(),
            peer_dependencies: HashMap::new(),
        }
    }
}

/// Type of dependency relationship
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyType {
    Production,
    Development,
    Optional,
    Peer,
}

/// Represents a dependency relationship edge
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub _dependency_type: DependencyType,
    pub _version_spec: String,
}

/// Dependency graph service
pub struct DependencyGraphService {
    /// Use directed graph to represent dependency relationships
    pub(crate) graph: DiGraph<PackageNode, DependencyEdge>,
    /// Mapping from package name to node index list (one package name may correspond to multiple nodes)
    name_to_indices: HashMap<String, Vec<NodeIndex>>,
    /// Mapping from path to node index
    path_to_index: HashMap<String, NodeIndex>,
}

impl Default for DependencyGraphService {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraphService {
    /// Create a new dependency graph service
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            name_to_indices: HashMap::new(),
            path_to_index: HashMap::new(),
        }
    }

    /// Add package node to the graph
    pub fn add_package(&mut self, package: PackageNode) -> Result<NodeIndex> {
        let package_name = package.name.clone();
        let package_path = package.path.clone();

        // Add node to the graph
        let node_index = self.graph.add_node(package);

        // Update mapping from package name to index list
        self.name_to_indices
            .entry(package_name)
            .or_default()
            .push(node_index);

        // Update mapping from path to index
        self.path_to_index.insert(package_path, node_index);

        Ok(node_index)
    }

    /// Find appropriate node index based on current path and package name
    /// 1. First find all nodeIndex based on name
    /// 2. Prefer to find nodeIndex for current path + node_modules/name
    /// 3. If found, return this nodeIndex
    /// 4. Otherwise, find all parent nodes of current path
    pub fn find_dependency_node_index(
        &self,
        current_path: &str,
        dependency_name: &str,
    ) -> Option<NodeIndex> {
        // 1. Find all node indices based on package name
        let candidate_indices = self.name_to_indices.get(dependency_name)?;

        if candidate_indices.is_empty() {
            return None;
        }

        // 2. Prefer to find node for current path + node_modules/dependency_name
        let preferred_path = if current_path.is_empty() {
            format!("node_modules/{dependency_name}")
        } else {
            format!("{current_path}/node_modules/{dependency_name}")
        };

        if let Some(&preferred_index) = self.path_to_index.get(&preferred_path) {
            return Some(preferred_index);
        }

        // 3. Find nodes in parent paths
        let mut search_path = current_path.to_string();
        loop {
            let candidate_path = if search_path.is_empty() {
                format!("node_modules/{dependency_name}")
            } else {
                format!("{search_path}/node_modules/{dependency_name}")
            };

            if let Some(&index) = self.path_to_index.get(&candidate_path) {
                return Some(index);
            }

            // Search upward for parent paths
            if search_path.is_empty() {
                break;
            }

            // Remove the last path segment
            if let Some(last_slash) = search_path.rfind('/') {
                search_path = search_path[..last_slash].to_string();
            } else {
                search_path.clear();
            }
        }

        // 4. If none found, return the first matching node
        candidate_indices.first().copied()
    }

    /// Add dependency relationship edge
    pub fn add_dependency_with_path(
        &mut self,
        from_path: &str,
        from_package_name: &str,
        to_package_name: &str,
        dependency_type: DependencyType,
        version_spec: String,
    ) -> Result<()> {
        // Find source package node
        let from_index = self
            .path_to_index
            .get(from_path)
            .context(format!("Package at path '{from_path}' not found in graph"))?;

        // Find target package node
        let to_index = self
            .find_dependency_node_index(from_path, to_package_name)
            .context(format!(
                "Dependency package '{to_package_name}' not found for path '{from_path}'"
            ))?;

        let edge = DependencyEdge {
            _dependency_type: dependency_type,
            _version_spec: version_spec,
        };

        log_verbose(&format!(
            "Adding dependency edge: {from_index:?} {from_package_name} -> {to_index:?} {to_package_name}"
        ));
        self.graph.add_edge(to_index, *from_index, edge);
        Ok(())
    }

    /// Helper DFS to find all paths between two nodes (NodeIndex version)
    fn dfs_all_paths_index(
        &self,
        current: NodeIndex,
        target: NodeIndex,
        visited: &mut Vec<NodeIndex>,
        path: &mut Vec<NodeIndex>,
        all_paths: &mut Vec<Vec<NodeIndex>>,
    ) {
        if visited.contains(&current) {
            return;
        }
        visited.push(current);
        path.push(current);
        if current == target {
            all_paths.push(path.clone());
        } else {
            for neighbor in self.graph.neighbors(current) {
                self.dfs_all_paths_index(neighbor, target, visited, path, all_paths);
            }
        }
        path.pop();
        visited.pop();
    }

    /// Find node indices by package name (supports fuzzy matching)
    pub fn find_package_indices_by_name(&self, name: &str) -> Vec<NodeIndex> {
        if let Some(indices) = self.name_to_indices.get(name) {
            indices.clone()
        } else {
            Vec::new()
        }
    }

    /// Find paths from specified package to root node (NodeIndex version)
    pub fn find_paths_to_root(&self, package_name: &str) -> Result<Vec<Vec<NodeIndex>>> {
        let package_indices = self.find_package_indices_by_name(package_name);

        if package_indices.is_empty() {
            return Err(anyhow::anyhow!("Package '{package_name}' not found"));
        }

        let mut all_paths = Vec::new();

        // Directly get root node (node with empty path)
        if let Some(&root_index) = self.path_to_index.get("") {
            // For each matching package node, find all paths to root node
            for &package_index in &package_indices {
                let paths = self.find_path_between_indices(package_index, root_index)?;
                for path in paths {
                    all_paths.push(path);
                }
            }
        }

        Ok(all_paths)
    }

    /// Find all paths between two node indices (NodeIndex version)
    fn find_path_between_indices(
        &self,
        from: NodeIndex,
        to: NodeIndex,
    ) -> Result<Vec<Vec<NodeIndex>>> {
        let mut all_paths = Vec::new();
        let mut path = Vec::new();
        let mut visited = Vec::new();
        self.dfs_all_paths_index(from, to, &mut visited, &mut path, &mut all_paths);
        Ok(all_paths)
    }

    pub fn get_graph(&self) -> &DiGraph<PackageNode, DependencyEdge> {
        &self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_graph_basic() {
        let mut graph = DependencyGraphService::new();

        // Add packages
        let package_a =
            PackageNode::new("package-a".to_string(), "1.0.0".to_string(), "".to_string());
        let package_b = PackageNode::new(
            "package-b".to_string(),
            "2.0.0".to_string(),
            "node_modules/package-b".to_string(),
        );

        let _index_a = graph.add_package(package_a).unwrap();
        let _index_b = graph.add_package(package_b).unwrap();

        assert_eq!(graph.graph.node_count(), 2);
    }

    #[test]
    fn test_dependency_path_resolution() {
        let mut graph = DependencyGraphService::new();

        // Create multiple packages with same name but different paths
        let root_package = PackageNode::new("app".to_string(), "1.0.0".to_string(), "".to_string());
        let lodash_root = PackageNode::new(
            "lodash".to_string(),
            "4.17.21".to_string(),
            "node_modules/lodash".to_string(),
        );
        let lodash_nested = PackageNode::new(
            "lodash".to_string(),
            "3.10.1".to_string(),
            "node_modules/some-package/node_modules/lodash".to_string(),
        );

        graph.add_package(root_package).unwrap();
        graph.add_package(lodash_root).unwrap();
        graph.add_package(lodash_nested).unwrap();

        // Test path resolution logic
        let root_lodash = graph.find_dependency_node_index("", "lodash");
        let nested_lodash = graph.find_dependency_node_index("node_modules/some-package", "lodash");

        assert!(root_lodash.is_some());
        assert!(nested_lodash.is_some());

        // Verify that the correct packages are found
        let root_lodash_node = graph.graph.node_weight(root_lodash.unwrap()).unwrap();
        let nested_lodash_node = graph.graph.node_weight(nested_lodash.unwrap()).unwrap();

        assert_eq!(root_lodash_node.path, "node_modules/lodash");
        assert_eq!(
            nested_lodash_node.path,
            "node_modules/some-package/node_modules/lodash"
        );
    }
}
