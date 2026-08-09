use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use petgraph::Graph;
use petgraph::graph::{DiGraph, NodeIndex};
use utoo_ruborist::lock::{LockPackageNode, PackageLock};

/// Upper bound on the number of dependency chains `ut list` enumerates per
/// (package, root) pair. Diamond dependencies make the full set exponential;
/// past this point more chains add noise, not insight.
const MAX_WHY_PATHS: usize = 1024;

/// Service for querying dependency graph built from package-lock.json.
/// This is different from ruborist's DependencyGraph which is used for resolution.
pub struct LockGraphService {
    /// Use directed graph to represent dependency relationships
    pub(crate) graph: DiGraph<LockPackageNode, ()>,
    /// Mapping from package name to node index list (one package name may correspond to multiple nodes)
    name_to_indices: std::collections::HashMap<String, Vec<NodeIndex>>,
    /// Mapping from path to node index
    path_to_index: std::collections::HashMap<String, NodeIndex>,
}

impl Default for LockGraphService {
    fn default() -> Self {
        Self::new()
    }
}

impl LockGraphService {
    /// Create a new lock graph service
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            name_to_indices: std::collections::HashMap::new(),
            path_to_index: std::collections::HashMap::new(),
        }
    }

    /// Load and build lock graph from package-lock.json file
    pub fn from_lock_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content =
            std::fs::read_to_string(path.as_ref()).context("Failed to read package-lock.json")?;
        let package_lock: PackageLock =
            serde_json::from_str(&content).context("Failed to parse package-lock.json")?;
        Self::from_package_lock(&package_lock)
    }

    /// Build lock graph from PackageLock
    pub fn from_package_lock(package_lock: &PackageLock) -> Result<Self> {
        let mut graph = Self::new();

        // First add all package entries
        for (path, package) in &package_lock.packages {
            let node = LockPackageNode::new(path.clone(), package.clone());
            tracing::debug!("Adding package: {node:?}");
            graph.add_package(node)?;
        }

        // Then add dependency relationships
        for (path, package) in &package_lock.packages {
            let name = package.get_name(path);

            // Add all dependency edges
            let all_deps = [
                &package.dependencies,
                &package.dev_dependencies,
                &package.optional_dependencies,
                &package.peer_dependencies,
            ];

            for deps in all_deps.into_iter().flatten() {
                for dep_name in deps.keys() {
                    if let Err(e) = graph.add_dependency_edge(path, &name, dep_name) {
                        tracing::debug!(
                            "Warning: Failed to add dependency {dep_name} for {name}: {e}"
                        );
                    }
                }
            }
        }

        Ok(graph)
    }

    /// Add package node to the graph
    pub fn add_package(&mut self, node: LockPackageNode) -> Result<NodeIndex> {
        let package_name = node.name();
        let package_path = node.path.clone();

        // Add node to the graph
        let node_index = self.graph.add_node(node);

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
    ///
    /// Resolution order:
    /// 1. Check current path + node_modules/dependency_name
    /// 2. Walk up the path hierarchy checking node_modules at each level
    /// 3. For workspace packages (paths without node_modules), also check root node_modules
    /// 4. Fall back to first matching node
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

        // 3. For workspace packages (paths without "node_modules"), check root node_modules first
        // This handles the common case where dependencies are hoisted to root
        if !current_path.is_empty() && !current_path.contains("node_modules") {
            let root_path = format!("node_modules/{dependency_name}");
            if let Some(&index) = self.path_to_index.get(&root_path) {
                return Some(index);
            }
        }

        // 4. Find nodes in parent paths
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

        // 5. If none found, return the first matching node
        candidate_indices.first().copied()
    }

    /// Add dependency relationship edge
    fn add_dependency_edge(
        &mut self,
        from_path: &str,
        from_package_name: &str,
        to_package_name: &str,
    ) -> Result<()> {
        // Find source package node
        let from_index = self
            .path_to_index
            .get(from_path)
            .with_context(|| format!("Package at path '{from_path}' not found in graph"))?;

        // Find target package node
        let to_index = self
            .find_dependency_node_index(from_path, to_package_name)
            .with_context(|| {
                format!("Dependency package '{to_package_name}' not found for path '{from_path}'")
            })?;

        tracing::debug!(
            "Adding dependency edge: {from_index:?} {from_package_name} -> {to_index:?} {to_package_name}"
        );
        self.graph.add_edge(to_index, *from_index, ());
        Ok(())
    }

    /// Helper DFS to find all paths between two nodes (NodeIndex version).
    ///
    /// All-simple-paths enumeration is exponential on diamond-heavy
    /// node_modules graphs, so collection stops at [`MAX_WHY_PATHS`]; the
    /// caller reports the truncation. Returns `false` when the cap was hit.
    fn dfs_all_paths_index(
        &self,
        current: NodeIndex,
        target: NodeIndex,
        visited: &mut std::collections::HashSet<NodeIndex>,
        path: &mut Vec<NodeIndex>,
        all_paths: &mut Vec<Vec<NodeIndex>>,
    ) -> bool {
        if all_paths.len() >= MAX_WHY_PATHS {
            return false;
        }
        if !visited.insert(current) {
            return true;
        }
        path.push(current);
        let mut complete = true;
        if current == target {
            all_paths.push(path.clone());
        } else {
            for neighbor in self.graph.neighbors(current) {
                if !self.dfs_all_paths_index(neighbor, target, visited, path, all_paths) {
                    complete = false;
                    break;
                }
            }
        }
        path.pop();
        visited.remove(&current);
        complete
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
    ///
    /// In workspace projects, paths may lead to:
    /// 1. The root package (path = "")
    /// 2. Workspace packages (path like "packages/xxx" without "node_modules")
    pub fn find_paths_to_root(&self, package_name: &str) -> Result<Vec<Vec<NodeIndex>>> {
        let package_indices = self.find_package_indices_by_name(package_name);

        if package_indices.is_empty() {
            return Err(anyhow::anyhow!(
                "Package '{package_name}' was not found in the lockfile.\n\
                 Run `utoo deps` to list installed packages, or `utoo install` to sync the lockfile."
            ));
        }

        let mut all_paths = Vec::new();

        // Collect all "root-like" nodes: root package and workspace packages
        // Workspace packages have paths that don't contain "node_modules"
        let root_indices: Vec<NodeIndex> = self
            .path_to_index
            .iter()
            .filter(|(path, _)| {
                // Root package has empty path
                // Workspace packages have non-empty paths without "node_modules"
                path.is_empty() || !path.contains("node_modules")
            })
            .map(|(_, &idx)| idx)
            .collect();

        // For each matching package node, find all paths to any root-like node
        for &package_index in &package_indices {
            for &root_index in &root_indices {
                let paths = self.find_path_between_indices(package_index, root_index)?;
                for path in paths {
                    all_paths.push(path);
                }
            }
        }

        Ok(all_paths)
    }

    /// Find all paths between two node indices (NodeIndex version).
    ///
    /// Capped at [`MAX_WHY_PATHS`]; logs a warning when paths were dropped so
    /// truncated output is never mistaken for the full set.
    fn find_path_between_indices(
        &self,
        from: NodeIndex,
        to: NodeIndex,
    ) -> Result<Vec<Vec<NodeIndex>>> {
        let mut all_paths = Vec::new();
        let mut path = Vec::new();
        let mut visited = std::collections::HashSet::new();
        if !self.dfs_all_paths_index(from, to, &mut visited, &mut path, &mut all_paths) {
            tracing::warn!(
                "dependency path listing truncated at {MAX_WHY_PATHS} paths; output is partial"
            );
        }
        Ok(all_paths)
    }

    pub fn get_graph(&self) -> &DiGraph<LockPackageNode, ()> {
        &self.graph
    }
}

/// A node in the dependency tree built from root-to-package paths.
///
/// `NodeIndex::end()` marks the synthetic root; real nodes index into the
/// lock graph.
#[derive(Debug)]
pub struct DepTreeNode {
    pub index: NodeIndex,
    pub children: BTreeMap<NodeIndex, DepTreeNode>,
}

/// Merge a set of package→root paths into a single tree rooted at a
/// synthetic `NodeIndex::end()` node.
pub fn build_dep_tree(paths: &[Vec<NodeIndex>]) -> DepTreeNode {
    let mut root = DepTreeNode {
        index: NodeIndex::end(),
        children: BTreeMap::new(),
    };
    for path in paths {
        let mut node = &mut root;
        for &idx in path.iter().rev() {
            node = node.children.entry(idx).or_insert_with(|| DepTreeNode {
                index: idx,
                children: BTreeMap::new(),
            });
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoo_ruborist::lock::LockPackage;

    fn make_lock_package(name: &str, version: &str) -> LockPackage {
        LockPackage {
            name: Some(name.to_string()),
            version: Some(version.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_dependency_graph_basic() {
        let mut graph = LockGraphService::new();

        // Add packages
        let package_a =
            LockPackageNode::new("".to_string(), make_lock_package("package-a", "1.0.0"));
        let package_b = LockPackageNode::new(
            "node_modules/package-b".to_string(),
            make_lock_package("package-b", "2.0.0"),
        );

        let _index_a = graph.add_package(package_a).unwrap();
        let _index_b = graph.add_package(package_b).unwrap();

        assert_eq!(graph.graph.node_count(), 2);
    }

    #[test]
    fn test_dependency_path_resolution() {
        let mut graph = LockGraphService::new();

        // Create multiple packages with same name but different paths
        let root_package = LockPackageNode::new("".to_string(), make_lock_package("app", "1.0.0"));
        let lodash_root = LockPackageNode::new(
            "node_modules/lodash".to_string(),
            make_lock_package("lodash", "4.17.21"),
        );
        let lodash_nested = LockPackageNode::new(
            "node_modules/some-package/node_modules/lodash".to_string(),
            make_lock_package("lodash", "3.10.1"),
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

    #[test]
    fn test_workspace_dependency_resolution() {
        let mut graph = LockGraphService::new();

        // Simulate workspace structure:
        // root ("") -> workspace package ("packages/my-app") -> lodash (hoisted to "node_modules/lodash")
        let root = LockPackageNode::new("".to_string(), make_lock_package("monorepo", "1.0.0"));
        let workspace_pkg = LockPackageNode::new(
            "packages/my-app".to_string(),
            make_lock_package("my-app", "1.0.0"),
        );
        let lodash = LockPackageNode::new(
            "node_modules/lodash".to_string(),
            make_lock_package("lodash", "4.17.21"),
        );

        graph.add_package(root).unwrap();
        graph.add_package(workspace_pkg).unwrap();
        graph.add_package(lodash).unwrap();

        // Workspace package should find hoisted lodash in root node_modules
        let ws_lodash = graph.find_dependency_node_index("packages/my-app", "lodash");
        assert!(ws_lodash.is_some());
        let ws_lodash_node = graph.graph.node_weight(ws_lodash.unwrap()).unwrap();
        assert_eq!(ws_lodash_node.path, "node_modules/lodash");
    }

    #[test]
    fn test_workspace_find_paths_to_root() {
        let mut graph = LockGraphService::new();

        // Build workspace graph:
        // root ("") has dependency on workspace package ("packages/my-app")
        // workspace package has dependency on lodash ("node_modules/lodash")
        let mut root_pkg = make_lock_package("monorepo", "1.0.0");
        root_pkg.dependencies = Some(
            [("my-app".to_string(), "*".to_string())]
                .into_iter()
                .collect(),
        );
        let root = LockPackageNode::new("".to_string(), root_pkg);

        let mut ws_pkg = make_lock_package("my-app", "1.0.0");
        ws_pkg.dependencies = Some(
            [("lodash".to_string(), "^4.17.0".to_string())]
                .into_iter()
                .collect(),
        );
        let workspace_pkg = LockPackageNode::new("packages/my-app".to_string(), ws_pkg);

        let lodash = LockPackageNode::new(
            "node_modules/lodash".to_string(),
            make_lock_package("lodash", "4.17.21"),
        );

        let root_idx = graph.add_package(root).unwrap();
        let ws_idx = graph.add_package(workspace_pkg).unwrap();
        let lodash_idx = graph.add_package(lodash).unwrap();

        // Add edges: lodash -> my-app -> root
        graph.graph.add_edge(lodash_idx, ws_idx, ());
        graph.graph.add_edge(ws_idx, root_idx, ());

        // Find paths from lodash to root should include the path through workspace
        let paths = graph.find_paths_to_root("lodash").unwrap();
        assert!(!paths.is_empty(), "Should find paths from lodash to root");

        // The path should include workspace package
        let path = &paths[0];
        assert!(path.len() >= 2, "Path should have at least 2 nodes");

        // Verify path goes through workspace package
        let has_workspace = path.iter().any(|&idx| {
            graph
                .get_graph()
                .node_weight(idx)
                .map(|n| n.path == "packages/my-app")
                .unwrap_or(false)
        });
        assert!(has_workspace, "Path should go through workspace package");
    }

    #[test]
    fn test_workspace_packages_are_root_like() {
        let mut graph = LockGraphService::new();

        // Create structure where lodash is only depended on by workspace package
        let root = LockPackageNode::new("".to_string(), make_lock_package("monorepo", "1.0.0"));
        let workspace_a = LockPackageNode::new(
            "packages/app-a".to_string(),
            make_lock_package("app-a", "1.0.0"),
        );
        let workspace_b = LockPackageNode::new(
            "packages/app-b".to_string(),
            make_lock_package("app-b", "1.0.0"),
        );
        let lodash = LockPackageNode::new(
            "node_modules/lodash".to_string(),
            make_lock_package("lodash", "4.17.21"),
        );

        let _root_idx = graph.add_package(root).unwrap();
        let ws_a_idx = graph.add_package(workspace_a).unwrap();
        let ws_b_idx = graph.add_package(workspace_b).unwrap();
        let lodash_idx = graph.add_package(lodash).unwrap();

        // lodash is used by both workspace packages
        graph.graph.add_edge(lodash_idx, ws_a_idx, ());
        graph.graph.add_edge(lodash_idx, ws_b_idx, ());

        // find_paths_to_root should find paths to both workspace packages
        let paths = graph.find_paths_to_root("lodash").unwrap();
        assert!(
            paths.len() >= 2,
            "Should find paths to multiple workspace packages"
        );
    }

    #[test]
    fn test_build_dep_tree() {
        let paths = vec![vec![
            NodeIndex::new(3),
            NodeIndex::new(2),
            NodeIndex::new(1),
            NodeIndex::new(0),
        ]];
        let tree = build_dep_tree(&paths);
        // root should have one child (a)
        assert_eq!(tree.children.len(), 1);
    }

    #[test]
    fn test_find_paths_to_root_not_found_includes_package_name() {
        let graph = LockGraphService::new();
        let result = graph.find_paths_to_root("nonexistent-package");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nonexistent-package"),
            "error should mention the queried package name, got: {err}"
        );
    }

    #[test]
    fn test_find_paths_to_root_not_found_suggests_actions() {
        let graph = LockGraphService::new();
        let result = graph.find_paths_to_root("nonexistent-package");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("utoo deps"),
            "error should suggest `utoo deps`, got: {err}"
        );
        assert!(
            err.contains("utoo install"),
            "error should suggest `utoo install`, got: {err}"
        );
    }
}
