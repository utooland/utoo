use petgraph::Direction::{Incoming, Outgoing};
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::node::{EdgeType, NodeType};
use super::override_rule::Overrides;
use crate::util::semver::matches;

/// Package node in the dependency graph
#[derive(Debug, Clone)]
pub struct PackageNode {
    pub path: PathBuf,
    pub name: String,
    pub version: String,
    pub package: Value,
    pub node_type: NodeType,
    pub is_prod: bool,
    pub is_dev: bool,
    pub is_peer: bool,
    pub is_optional: bool,
}

impl PackageNode {
    pub fn new(name: String, path: PathBuf, package: Value) -> Self {
        let version = package
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Self {
            path,
            name,
            version,
            package,
            node_type: NodeType::Regular,
            is_prod: false,
            is_dev: false,
            is_peer: false,
            is_optional: false,
        }
    }

    pub fn new_root(name: String, path: PathBuf, package: Value) -> Self {
        let version = package
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Self {
            path,
            name,
            version,
            package,
            node_type: NodeType::Root,
            is_prod: false,
            is_dev: false,
            is_peer: false,
            is_optional: false,
        }
    }

    pub fn new_workspace(name: String, path: PathBuf, package: Value) -> Self {
        let version = package
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();

        Self {
            path,
            name,
            version,
            package,
            node_type: NodeType::Workspace,
            is_prod: false,
            is_dev: false,
            is_peer: false,
            is_optional: false,
        }
    }

    pub fn new_link(name: String, path: PathBuf, package: Value, version: String) -> Self {
        Self {
            path,
            name,
            version,
            package,
            node_type: NodeType::Link,
            is_prod: false,
            is_dev: false,
            is_peer: false,
            is_optional: false,
        }
    }

    pub fn is_root(&self) -> bool {
        self.node_type == NodeType::Root
    }

    pub fn is_workspace(&self) -> bool {
        self.node_type == NodeType::Workspace
    }

    pub fn is_link(&self) -> bool {
        self.node_type == NodeType::Link
    }
}

/// Edge in the dependency graph
#[derive(Debug, Clone)]
pub enum GraphEdge {
    Physical,
    Dependency(DependencyEdge),
}

/// Dependency edge data
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub name: String,
    pub spec: String,
    pub edge_type: EdgeType,
    pub valid: bool,
    pub to: Option<NodeIndex>,
}

impl DependencyEdge {
    pub fn new(name: String, spec: String, edge_type: EdgeType) -> Self {
        Self {
            name,
            spec: if spec.trim().is_empty() {
                "*".to_string()
            } else {
                spec
            },
            edge_type,
            valid: false,
            to: None,
        }
    }
}

/// The main dependency graph structure
pub struct DependencyGraph {
    pub graph: DiGraph<PackageNode, GraphEdge>,
    pub root_index: NodeIndex,
    // Project-level overrides configuration
    overrides: Option<Overrides>,
    // Fast lookup set for override names
    override_names: std::collections::HashSet<String>,
}

impl DependencyGraph {
    /// Create a new dependency graph with a root node
    pub fn new(path: PathBuf, package: Value) -> Self {
        let mut graph = DiGraph::new();
        let name = package
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("root")
            .to_string();

        // Parse overrides from package.json
        let overrides = Overrides::parse(package.clone());

        // Extract override names for fast lookup
        let override_names = if let Some(ref rules) = overrides {
            let names: std::collections::HashSet<String> =
                rules.rules.iter().map(|r| r.name.clone()).collect();
            tracing::debug!("Parsed {} override rules", rules.rules.len());
            for rule in &rules.rules {
                tracing::debug!(
                    "  Rule: {}@{} -> {}, parent: {:?}",
                    rule.name,
                    rule.spec,
                    rule.target_spec,
                    rule.parent
                        .as_ref()
                        .map(|p| format!("{}@{}", p.name, p.spec))
                );
            }
            names
        } else {
            std::collections::HashSet::new()
        };

        let root_node = PackageNode::new_root(name, path, package);
        let root_index = graph.add_node(root_node);

        Self {
            graph,
            root_index,
            overrides,
            override_names,
        }
    }

    /// Add a package node to the graph
    pub fn add_node(&mut self, node: PackageNode) -> NodeIndex {
        self.graph.add_node(node)
    }

    /// Add a physical parent-child edge
    pub fn add_physical_edge(&mut self, parent: NodeIndex, child: NodeIndex) -> EdgeIndex {
        self.graph.add_edge(parent, child, GraphEdge::Physical)
    }

    /// Add a dependency edge
    pub fn add_dependency_edge(
        &mut self,
        from: NodeIndex,
        name: String,
        spec: String,
        edge_type: EdgeType,
    ) -> EdgeIndex {
        let dep_edge = DependencyEdge::new(name, spec, edge_type);
        self.graph
            .add_edge(from, from, GraphEdge::Dependency(dep_edge))
    }

    /// Get the physical parent of a node
    pub fn get_physical_parent(&self, node: NodeIndex) -> Option<NodeIndex> {
        self.graph
            .edges_directed(node, Incoming)
            .find(|edge| matches!(edge.weight(), GraphEdge::Physical))
            .map(|edge| edge.source())
    }

    /// Get all physical children of a node
    pub fn get_physical_children(&self, node: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .edges_directed(node, Outgoing)
            .filter(|edge| matches!(edge.weight(), GraphEdge::Physical))
            .map(|edge| edge.target())
            .collect()
    }

    /// Get all dependency edges from a node
    pub fn get_dependency_edges(&self, node: NodeIndex) -> Vec<(EdgeIndex, &DependencyEdge)> {
        self.graph
            .edges_directed(node, Outgoing)
            .filter_map(|edge| {
                if let GraphEdge::Dependency(dep) = edge.weight() {
                    Some((edge.id(), dep))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Mark a dependency edge as resolved
    pub fn mark_dependency_resolved(&mut self, edge_id: EdgeIndex, target: NodeIndex) {
        if let Some(GraphEdge::Dependency(dep)) = self.graph.edge_weight_mut(edge_id) {
            dep.valid = true;
            dep.to = Some(target);
        }
    }

    /// Get node by index
    pub fn get_node(&self, index: NodeIndex) -> Option<&PackageNode> {
        self.graph.node_weight(index)
    }

    /// Get mutable node by index
    pub fn get_node_mut(&mut self, index: NodeIndex) -> Option<&mut PackageNode> {
        self.graph.node_weight_mut(index)
    }

    /// Get all workspace nodes (excluding links)
    pub fn get_workspace_nodes(&self) -> Vec<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                if let Some(node) = self.get_node(idx) {
                    node.is_workspace() && !node.is_link()
                } else {
                    false
                }
            })
            .collect()
    }

    /// Get all resolved dependency targets for a node
    /// Returns Vec<(dependency_name, target_node_index)>
    pub fn get_resolved_dependencies(&self, node_index: NodeIndex) -> Vec<(String, NodeIndex)> {
        self.get_dependency_edges(node_index)
            .into_iter()
            .filter_map(|(_, dep)| {
                if dep.valid {
                    dep.to.map(|target| (dep.name.clone(), target))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Serialize graph to package-lock.json format
    pub fn serialize_to_packages(&self, root_path: &Path) -> (Value, i32) {
        let mut packages = json!({});
        let mut stack = vec![(self.root_index, String::new())];
        let mut total_packages = 0;

        while let Some((node_index, prefix)) = stack.pop() {
            let _node = &self.graph[node_index];

            // Check for duplicate dependencies
            self.check_duplicate_dependencies(node_index);

            // Create package info
            let pkg_info = self.create_package_info(node_index, root_path, &mut total_packages);

            // Use empty string for root node
            let key = if prefix.is_empty() {
                String::new()
            } else {
                prefix.clone()
            };
            packages[key] = pkg_info;

            // Add physical children to processing stack
            for child_index in self.get_physical_children(node_index) {
                let child = &self.graph[child_index];
                let child_prefix = if prefix.is_empty() {
                    if child.is_workspace() {
                        // Workspace nodes use relative path from root
                        child
                            .path
                            .strip_prefix(root_path)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| child.path.to_string_lossy().to_string())
                    } else {
                        format!("node_modules/{}", child.name)
                    }
                } else {
                    format!("{}/node_modules/{}", prefix, child.name)
                };
                stack.push((child_index, child_prefix));
            }
        }

        (packages, total_packages)
    }

    /// Check for duplicate dependencies under a node and log warnings
    fn check_duplicate_dependencies(&self, node_index: NodeIndex) {
        let mut name_count = HashMap::new();
        for child_index in self.get_physical_children(node_index) {
            let child = &self.graph[child_index];
            if !child.is_link() {
                *name_count.entry(child.name.as_str()).or_insert(0) += 1;
            }
        }
        for (name, count) in name_count {
            if count > 1 {
                let node = &self.graph[node_index];
                tracing::warn!(
                    "Found {} duplicate dependencies named '{}' under '{}'",
                    count,
                    name,
                    node.name
                );
            }
        }
    }

    /// Create package information for a node
    fn create_package_info(
        &self,
        node_index: NodeIndex,
        root_path: &Path,
        total_packages: &mut i32,
    ) -> Value {
        let node = &self.graph[node_index];

        if node.is_root() {
            // Root node: all fields are handled in create_root_package_info
            self.create_root_package_info(node_index)
        } else {
            // Non-root nodes: create basic info then add package fields
            let mut pkg_info =
                self.create_non_root_package_info(node_index, root_path, total_packages);
            self.add_package_fields(&mut pkg_info, node_index);
            pkg_info
        }
    }

    /// Create package info for root node
    fn create_root_package_info(&self, node_index: NodeIndex) -> Value {
        let node = &self.graph[node_index];
        let mut info = json!({
            "name": node.name,
            "version": node.version,
        });

        // Add optional fields from package.json
        if let Some(engines) = node.package.get("engines") {
            info["engines"] = engines.clone();
        }
        if let Some(workspaces) = node.package.get("workspaces") {
            info["workspaces"] = workspaces.clone();
        }

        // Add dependency fields directly from package.json (not from graph edges)
        // This ensures workspace dependencies are not included
        let dep_fields = vec![
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ];
        for field in dep_fields {
            if let Some(deps) = node.package.get(field)
                && deps.is_object()
                && !deps.as_object().unwrap().is_empty()
            {
                info[field] = deps.clone();
            }
        }

        info
    }

    /// Create package info for non-root nodes
    fn create_non_root_package_info(
        &self,
        node_index: NodeIndex,
        root_path: &Path,
        total_packages: &mut i32,
    ) -> Value {
        let node = &self.graph[node_index];
        let mut info = json!({
            "name": node.package.get("name"),
        });

        if node.is_workspace() {
            info["version"] = json!(node.package.get("version"));
        } else if node.is_link() {
            info["link"] = json!(true);
            let relative_path = self.get_relative_path(&node.path, root_path);
            info["resolved"] = json!(relative_path);
        } else {
            // Regular package
            *total_packages += 1;
            info["version"] = json!(node.package.get("version"));

            // Get resolved and integrity from dist field
            let empty_dist = json!("");
            let dist = node.package.get("dist").unwrap_or(&empty_dist);
            info["resolved"] = json!(dist.get("tarball"));
            info["integrity"] = json!(dist.get("integrity"));
        }

        // Add optional flags (dev, optional, peer)
        self.add_optional_flags(&mut info, node_index);

        info
    }

    /// Add optional flags (peer, dev, optional, hasInstallScript)
    fn add_optional_flags(&self, info: &mut Value, node_index: NodeIndex) {
        let node = &self.graph[node_index];

        if node.is_peer {
            info["peer"] = json!(true);
        }

        match (node.is_dev, node.is_optional) {
            (true, true) => info["devOptional"] = json!(true),
            (true, false) => info["dev"] = json!(true),
            (false, true) => info["optional"] = json!(true),
            _ => {}
        }

        if node.package.get("hasInstallScript") == Some(&json!(true)) {
            info["hasInstallScript"] = json!(true);
        }
    }

    /// Add package fields like dependencies, bin, license, etc.
    fn add_package_fields(&self, pkg_info: &mut Value, node_index: NodeIndex) {
        let node = &self.graph[node_index];

        // Add various package.json fields
        let fields = vec![
            "bin",
            "license",
            "engines",
            "os",
            "cpu",
            "scripts",
            "hasInstallScript",
        ];

        for field in fields {
            if let Some(value) = node.package.get(field) {
                pkg_info[field] = value.clone();
            }
        }

        // Add dependency fields
        self.add_dependency_fields(pkg_info, node_index);
    }

    /// Add dependency fields to package info
    fn add_dependency_fields(&self, pkg_info: &mut Value, node_index: NodeIndex) {
        let dep_types = vec![
            ("dependencies", EdgeType::Prod),
            ("devDependencies", EdgeType::Dev),
            ("peerDependencies", EdgeType::Peer),
            ("optionalDependencies", EdgeType::Optional),
        ];

        for (field_name, edge_type) in dep_types {
            let deps = self.collect_dependencies(node_index, &edge_type);
            if !deps.is_empty() {
                pkg_info[field_name] = json!(deps);
            }
        }
    }

    /// Collect dependencies of a specific type
    fn collect_dependencies(
        &self,
        node_index: NodeIndex,
        edge_type: &EdgeType,
    ) -> HashMap<String, String> {
        let mut deps = HashMap::new();

        for (_, dep_edge) in self.get_dependency_edges(node_index) {
            if &dep_edge.edge_type == edge_type {
                deps.insert(dep_edge.name.clone(), dep_edge.spec.clone());
            }
        }

        deps
    }

    /// Get relative path from root
    fn get_relative_path(&self, path: &Path, root_path: &Path) -> String {
        path.strip_prefix(root_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    /// Find compatible node in parent chain for dependency resolution
    pub fn find_compatible_node(
        &self,
        from: NodeIndex,
        name: &str,
        version_spec: &str,
    ) -> FindResult {
        // Check if this dependency has an override
        let effective_spec =
            if let Some(overridden_spec) = self.check_override(from, name, version_spec) {
                tracing::debug!(
                    "Using override spec for {}@{} => {}",
                    name,
                    version_spec,
                    overridden_spec
                );
                overridden_spec
            } else {
                version_spec.to_string()
            };

        // Get physical parent of from node, default to from if it's the root
        let parent = self.get_physical_parent(from).unwrap_or(from);

        // Recursively search up the parent chain with effective spec
        self.find_in_parent_chain(parent, name, &effective_spec, from)
    }

    /// Recursively search for compatible node in parent chain
    fn find_in_parent_chain(
        &self,
        current: NodeIndex,
        name: &str,
        spec: &str,
        requester: NodeIndex,
    ) -> FindResult {
        // Check all physical children of current node
        for child_idx in self.get_physical_children(current) {
            let child = &self.graph[child_idx];
            if child.name == name {
                if matches(spec, &child.version) {
                    tracing::debug!(
                        "found existing deps {}@{} got {}, reuse at {:?}",
                        name,
                        spec,
                        child.version,
                        child_idx
                    );
                    return FindResult::Reuse(child_idx);
                } else {
                    tracing::debug!(
                        "found conflict deps {}@{} got {}, conflict at {:?}",
                        name,
                        spec,
                        child.version,
                        child_idx
                    );
                    return FindResult::Conflict(requester);
                }
            }
        }

        // Recurse to parent
        if let Some(parent) = self.get_physical_parent(current) {
            self.find_in_parent_chain(parent, name, spec, requester)
        } else {
            // Reached root, install here
            FindResult::New(current)
        }
    }

    /// Collect the physical parent chain from a node up to root
    /// Includes the 'from' node itself in the chain
    /// Returns Vec<(name, version)> for version-aware matching
    fn collect_parent_chain(&self, from: NodeIndex) -> Vec<(String, String)> {
        let mut chain = Vec::new();
        let mut current = from;

        // First, add the current node itself (if not root)
        let from_node = &self.graph[from];
        if !from_node.is_root() {
            chain.push((from_node.name.clone(), from_node.version.clone()));
        }

        // Then collect all physical parents up to root
        while let Some(parent) = self.get_physical_parent(current) {
            let parent_node = &self.graph[parent];
            if !parent_node.is_root() {
                chain.push((parent_node.name.clone(), parent_node.version.clone()));
            }
            current = parent;
        }

        chain.reverse();
        chain
    }

    /// Check if an override rule applies and return the overridden spec
    pub fn check_override(&self, from: NodeIndex, name: &str, spec: &str) -> Option<String> {
        // Fast path: skip if name is not in override_names
        if !self.override_names.contains(name) {
            return None;
        }

        let overrides = self.overrides.as_ref()?;

        // Collect parent chain once
        let parent_chain = self.collect_parent_chain(from);

        // Match override rules
        for rule in &overrides.rules {
            if rule.name != name {
                continue;
            }

            // Check if spec matches (handle wildcard)
            if rule.spec != "*" && !matches(&rule.spec, spec) {
                continue;
            }

            // Check if parent chain matches
            if self.matches_parent_chain_for_rule(rule, &parent_chain) {
                tracing::debug!(
                    "Override matched: {}@{} => {} (from {:?}, chain: {:?})",
                    name,
                    spec,
                    rule.target_spec,
                    from,
                    parent_chain
                );
                return Some(rule.target_spec.clone());
            }
        }

        None
    }

    /// Check if a parent chain matches an override rule's parent condition
    /// Based on the original implementation in override_rule.rs
    fn matches_parent_chain_for_rule(
        &self,
        rule: &crate::model::override_rule::OverrideRule,
        parent_chain: &[(String, String)],
    ) -> bool {
        // If no parent requirement, always matches
        if rule.parent.is_none() {
            return true;
        }

        let mut current_rule = rule.parent.as_ref();
        let mut parent_idx = 0;

        while let Some((parent_name, parent_version)) = parent_chain.get(parent_idx) {
            if let Some(rule_ref) = current_rule
                && parent_name == &rule_ref.name
            {
                // Check if version matches
                let version_matches = if rule_ref.spec == "*" {
                    true
                } else {
                    matches(&rule_ref.spec, parent_version)
                };

                if version_matches {
                    // Move to next parent requirement
                    if let Some(next_rule) = rule_ref.parent.as_ref() {
                        current_rule = Some(next_rule);
                        parent_idx += 1;
                        continue;
                    } else {
                        // All parent requirements matched
                        return true;
                    }
                }
            }
            parent_idx += 1;
        }

        // If current_rule is still Some, it means we didn't match all parent requirements
        current_rule.is_none()
    }
}

/// Result of finding a compatible node
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindResult {
    /// Can reuse existing node
    Reuse(NodeIndex),
    /// Conflict found, install under this parent
    Conflict(NodeIndex),
    /// Need to install under this parent (usually root)
    New(NodeIndex),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_create_graph() {
        let pkg = json!({
            "name": "test",
            "version": "1.0.0"
        });
        let graph = DependencyGraph::new(PathBuf::from("."), pkg);
        assert_eq!(graph.graph.node_count(), 1);
        assert!(graph.graph[graph.root_index].is_root());
    }

    #[test]
    fn test_add_nodes_and_edges() {
        let pkg = json!({
            "name": "root",
            "version": "1.0.0"
        });
        let mut graph = DependencyGraph::new(PathBuf::from("."), pkg);

        let child_pkg = json!({
            "name": "lodash",
            "version": "4.17.21"
        });
        let child = PackageNode::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            child_pkg,
        );
        let child_idx = graph.add_node(child);

        graph.add_physical_edge(graph.root_index, child_idx);

        assert_eq!(graph.graph.node_count(), 2);
        assert_eq!(graph.get_physical_children(graph.root_index).len(), 1);
        assert_eq!(graph.get_physical_parent(child_idx), Some(graph.root_index));
    }

    #[test]
    fn test_dependency_edges() {
        let pkg = json!({
            "name": "root",
            "version": "1.0.0"
        });
        let mut graph = DependencyGraph::new(PathBuf::from("."), pkg);

        let edge_id = graph.add_dependency_edge(
            graph.root_index,
            "lodash".to_string(),
            "^4.17.0".to_string(),
            EdgeType::Prod,
        );

        let deps = graph.get_dependency_edges(graph.root_index);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].1.name, "lodash");
        assert_eq!(deps[0].1.spec, "^4.17.0");
        assert!(!deps[0].1.valid);

        // Mark as resolved
        let child_pkg = json!({
            "name": "lodash",
            "version": "4.17.21"
        });
        let child = PackageNode::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            child_pkg,
        );
        let child_idx = graph.add_node(child);
        graph.mark_dependency_resolved(edge_id, child_idx);

        let deps = graph.get_dependency_edges(graph.root_index);
        assert!(deps[0].1.valid);
        assert_eq!(deps[0].1.to, Some(child_idx));
    }

    #[test]
    fn test_find_compatible_node_reuse() {
        let pkg = json!({"name": "root", "version": "1.0.0"});
        let mut graph = DependencyGraph::new(PathBuf::from("."), pkg);

        // Add lodash@4.17.21 under root
        let lodash = PackageNode::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            json!({"name": "lodash", "version": "4.17.21"}),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // Should reuse existing lodash when spec matches
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::Reuse(lodash_idx));
    }

    #[test]
    fn test_find_compatible_node_conflict() {
        let pkg = json!({"name": "root", "version": "1.0.0"});
        let mut graph = DependencyGraph::new(PathBuf::from("."), pkg);

        // Add lodash@3.10.1 under root
        let lodash = PackageNode::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            json!({"name": "lodash", "version": "3.10.1"}),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // Should find conflict when spec doesn't match
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::Conflict(graph.root_index));
    }

    #[test]
    fn test_find_compatible_node_new() {
        let pkg = json!({"name": "root", "version": "1.0.0"});
        let graph = DependencyGraph::new(PathBuf::from("."), pkg);

        // Should return New when no existing node found
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::New(graph.root_index));
    }

    #[test]
    fn test_find_compatible_node_nested() {
        let pkg = json!({"name": "root", "version": "1.0.0"});
        let mut graph = DependencyGraph::new(PathBuf::from("."), pkg);

        // Add express under root
        let express = PackageNode::new(
            "express".to_string(),
            PathBuf::from("node_modules/express"),
            json!({"name": "express", "version": "4.18.0"}),
        );
        let express_idx = graph.add_node(express);
        graph.add_physical_edge(graph.root_index, express_idx);

        // Add lodash@4.17.21 under root
        let lodash = PackageNode::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            json!({"name": "lodash", "version": "4.17.21"}),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // From express, should find lodash in parent (root)
        let result = graph.find_compatible_node(express_idx, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::Reuse(lodash_idx));
    }
}
