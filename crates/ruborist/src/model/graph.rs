//! Dependency graph data structure using petgraph.

use petgraph::Direction::{Incoming, Outgoing};
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use std::sync::Arc;

use super::manifest::{CoreVersionManifest, NodeManifest};
use super::node::{EdgeType, NodeType};
use super::override_rule::Overrides;
use super::package_json::PackageJson;
use crate::resolver::semver::matches;

/// Package node in the dependency graph.
#[derive(Debug, Clone)]
pub struct PackageNode {
    /// Installation path relative to project root
    pub path: PathBuf,
    /// Package name
    pub name: String,
    /// Resolved version
    pub version: String,
    /// Package manifest (local or registry)
    pub manifest: NodeManifest,
    /// Type of node (Root, Regular, Workspace, Link)
    pub node_type: NodeType,
    /// Is this a production dependency
    pub is_prod: bool,
    /// Is this a development dependency
    pub is_dev: bool,
    /// Is this a peer dependency
    pub is_peer: bool,
    /// Is this an optional dependency
    pub is_optional: bool,
}

impl PackageNode {
    /// Core constructor: every node starts with all dependency-type flags
    /// clear (they are assigned later by `compute_node_types`). The public
    /// constructors below differ only in manifest source, node type, and how
    /// name/version are derived.
    fn new(
        name: String,
        version: String,
        path: PathBuf,
        manifest: NodeManifest,
        node_type: NodeType,
    ) -> Self {
        Self {
            path,
            name,
            version,
            manifest,
            node_type,
            is_prod: false,
            is_dev: false,
            is_peer: false,
            is_optional: false,
        }
    }

    /// Create a new regular package node from CoreVersionManifest.
    pub fn from_version_manifest(
        name: String,
        path: PathBuf,
        manifest: Arc<CoreVersionManifest>,
    ) -> Self {
        let version = manifest.version.clone();
        Self::new(
            name,
            version,
            path,
            NodeManifest::Registry(manifest),
            NodeType::Regular,
        )
    }

    /// Create a new regular package node from PackageJson.
    pub fn from_package_json(name: String, path: PathBuf, pkg: PackageJson) -> Self {
        let version = pkg.version.clone();
        Self::new(
            name,
            version,
            path,
            NodeManifest::Local(Box::new(pkg)),
            NodeType::Regular,
        )
    }

    /// Create a root project node from PackageJson.
    pub fn root_from_package_json(path: PathBuf, pkg: PackageJson) -> Self {
        let name = pkg.name.clone();
        let version = pkg.version.clone();
        Self::new(
            name,
            version,
            path,
            NodeManifest::Local(Box::new(pkg)),
            NodeType::Root,
        )
    }

    /// Create a workspace package node from PackageJson.
    pub fn workspace_from_package_json(path: PathBuf, pkg: PackageJson) -> Self {
        let name = pkg.name.clone();
        // A workspace member with no version pins to `*` so dependents can
        // always satisfy a `workspace:*` requirement.
        let version = if pkg.version.is_empty() {
            "*".to_string()
        } else {
            pkg.version.clone()
        };
        Self::new(
            name,
            version,
            path,
            NodeManifest::Local(Box::new(pkg)),
            NodeType::Workspace,
        )
    }

    /// Create a symlinked package node from PackageJson.
    pub fn link_from_package_json(path: PathBuf, pkg: PackageJson) -> Self {
        let name = pkg.name.clone();
        let version = pkg.version.clone();
        Self::new(
            name,
            version,
            path,
            NodeManifest::Local(Box::new(pkg)),
            NodeType::Link,
        )
    }

    /// Check if this is the root node.
    pub fn is_root(&self) -> bool {
        self.node_type == NodeType::Root
    }

    /// Check if this is a workspace node.
    pub fn is_workspace(&self) -> bool {
        self.node_type == NodeType::Workspace
    }

    /// Check if this is a symlinked node.
    pub fn is_link(&self) -> bool {
        self.node_type == NodeType::Link
    }

    /// Get the package manifest reference.
    pub fn get_manifest(&self) -> &NodeManifest {
        &self.manifest
    }
}

/// Edge type in the dependency graph.
#[derive(Debug, Clone)]
pub enum GraphEdge {
    /// Physical parent-child relationship (directory structure)
    Physical,
    /// Logical dependency relationship. Boxed: physical edges outnumber
    /// dependency edges in the petgraph and a unit variant must not pay the
    /// full DependencyEdge footprint.
    Dependency(Box<DependencyEdge>),
}

/// Dependency edge data.
#[derive(Debug, Clone)]
pub struct DependencyEdge {
    /// Dependency name
    pub name: String,
    /// Version specification
    pub spec: String,
    /// Type of dependency
    pub edge_type: EdgeType,
    /// Whether this edge has been resolved
    pub valid: bool,
    /// Target node index if resolved
    pub to: Option<NodeIndex>,
}

impl DependencyEdge {
    /// Create a new unresolved dependency edge.
    pub fn new(name: impl Into<String>, spec: impl Into<String>, edge_type: EdgeType) -> Self {
        let spec_str = spec.into();
        Self {
            name: name.into(),
            spec: if spec_str.trim().is_empty() {
                "*".to_string()
            } else {
                spec_str
            },
            edge_type,
            valid: false,
            to: None,
        }
    }
}

/// The main dependency graph structure.
pub struct DependencyGraph {
    /// The underlying petgraph structure
    pub graph: DiGraph<PackageNode, GraphEdge>,
    /// Index of the root node
    pub root_index: NodeIndex,
    /// Project-level overrides configuration
    pub(crate) overrides: Option<Overrides>,
    /// Fast lookup set for override names
    pub(crate) override_names: HashSet<String>,
    /// Per-parent `name → child` index over physical edges, maintained by
    /// [`add_physical_edge`](Self::add_physical_edge) (the graph is
    /// append-only — no edge ever gets removed). Hoisting parks most packages
    /// directly under root, so the parent-chain search would otherwise scan
    /// every root child (string compare each) for every edge — tens of
    /// millions of comparisons on large trees, all inside the single-threaded
    /// resolver driver.
    child_index: HashMap<NodeIndex, HashMap<String, NodeIndex>>,
    /// Workspace members by package name, registered in
    /// [`add_node`](Self::add_node) — the single node-creation chokepoint —
    /// so `workspace:` edge settlement never re-derives the member set from
    /// physical children (which would also have to dodge the same-name link
    /// node `add_workspace_member` attaches alongside each member).
    workspace_members: HashMap<String, NodeIndex>,
}

impl DependencyGraph {
    /// Create a new dependency graph with a root node from PackageJson.
    pub fn from_package_json(path: PathBuf, pkg: PackageJson) -> Self {
        let mut graph = DiGraph::new();

        // Parse overrides from package.json
        // Need to pass the full package.json for $dep_name reference resolution
        let overrides = Overrides::parse(pkg.to_value());

        // Extract override names for fast lookup
        let override_names = if let Some(ref rules) = overrides {
            let names: HashSet<String> = rules.rules.iter().map(|r| r.name.clone()).collect();
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
            HashSet::new()
        };

        let root_node = PackageNode::root_from_package_json(path, pkg);
        let root_index = graph.add_node(root_node);

        Self {
            graph,
            root_index,
            overrides,
            override_names,
            child_index: HashMap::new(),
            workspace_members: HashMap::new(),
        }
    }

    /// Add a package node to the graph.
    pub fn add_node(&mut self, node: PackageNode) -> NodeIndex {
        let is_workspace = node.is_workspace();
        let name = is_workspace.then(|| node.name.clone());
        let idx = self.graph.add_node(node);
        // Register workspace members at the single node-creation chokepoint,
        // so `workspace_members` is correct for every construction path
        // (install graph init, lockfile load, tests) with no rebuild pass.
        if let Some(name) = name {
            self.workspace_members.insert(name, idx);
        }
        idx
    }

    /// Workspace members by package name, maintained by [`add_node`](Self::add_node).
    pub fn workspace_members(&self) -> &HashMap<String, NodeIndex> {
        &self.workspace_members
    }

    /// Add a physical parent-child edge.
    pub fn add_physical_edge(&mut self, parent: NodeIndex, child: NodeIndex) -> EdgeIndex {
        // `insert` (last wins) mirrors petgraph's reverse-insertion edge
        // iteration the linear scan used to see first. Duplicate names under
        // one parent DO occur: `add_workspace_member` attaches a workspace
        // node and its link node under root with the same name — last-wins
        // keeps the link node, exactly what the old scan found first.
        let name = self.graph[child].name.clone();
        self.child_index
            .entry(parent)
            .or_default()
            .insert(name, child);
        self.graph.add_edge(parent, child, GraphEdge::Physical)
    }

    /// O(1) lookup of a physical child by name (see `child_index`).
    fn find_physical_child(&self, parent: NodeIndex, name: &str) -> Option<NodeIndex> {
        self.child_index.get(&parent)?.get(name).copied()
    }

    /// Whether any workspace member is attached under root — `workspace:`
    /// specs are only meaningful between members of a workspace project.
    /// Add a dependency edge (self-loop for tracking).
    pub fn add_dependency_edge(
        &mut self,
        from: NodeIndex,
        name: impl Into<String>,
        spec: impl Into<String>,
        edge_type: EdgeType,
    ) -> EdgeIndex {
        let dep_edge = DependencyEdge::new(name, spec, edge_type);
        self.graph
            .add_edge(from, from, GraphEdge::Dependency(Box::new(dep_edge)))
    }

    /// Get the physical parent of a node.
    pub fn get_physical_parent(&self, node: NodeIndex) -> Option<NodeIndex> {
        self.graph
            .edges_directed(node, Incoming)
            .find(|edge| matches!(edge.weight(), GraphEdge::Physical))
            .map(|edge| edge.source())
    }

    /// Collect the logical dependency ancestry of a node in root→`from` order (inclusive).
    ///
    /// Walks the "required by" chain — for each node, finds a node whose resolved
    /// dependency points at it, and continues until reaching the root (or hitting
    /// a cycle). This is *logical* ancestry: which package declared the dependency.
    /// It differs from the *physical* tree (install location), where hoisted
    /// packages all sit directly under root regardless of who required them.
    ///
    /// Each entry is `(name, version)`. Used to report which dependency chain
    /// introduced a failing package.
    pub(crate) fn logical_ancestry(&self, from: NodeIndex) -> Vec<(String, String)> {
        let requester = self.build_requester_index();

        let mut chain: Vec<(String, String)> = std::iter::successors(Some(from), |&idx| {
            requester
                .get(&idx)
                .copied()
                .filter(|_| idx != self.root_index)
        })
        .scan(HashSet::new(), |seen, idx| seen.insert(idx).then_some(idx))
        .map(|idx| {
            let node = &self.graph[idx];
            (node.name.clone(), node.version.clone())
        })
        .collect();
        chain.reverse();
        chain
    }

    /// Reverse index: resolved dep target → first depender encountered.
    ///
    /// Scanning every dep edge is `O(E)`; fine because the only caller
    /// (`logical_ancestry`) runs on the error path.
    fn build_requester_index(&self) -> HashMap<NodeIndex, NodeIndex> {
        let mut index = HashMap::new();
        for edge in self.graph.edge_references() {
            let GraphEdge::Dependency(dep) = edge.weight() else {
                continue;
            };
            let Some(target) = dep.to else { continue };
            let src = edge.source();
            if target == src {
                continue;
            }
            index.entry(target).or_insert(src);
        }
        index
    }

    /// Get all physical children of a node.
    pub fn get_physical_children(&self, node: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .edges_directed(node, Outgoing)
            .filter(|edge| matches!(edge.weight(), GraphEdge::Physical))
            .map(|edge| edge.target())
            .collect()
    }

    /// Get all dependency edges from a node.
    pub fn get_dependency_edges(&self, node: NodeIndex) -> Vec<(EdgeIndex, &DependencyEdge)> {
        self.graph
            .edges_directed(node, Outgoing)
            .filter_map(|edge| {
                if let GraphEdge::Dependency(dep) = edge.weight() {
                    Some((edge.id(), dep.as_ref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if a dependency edge resolves to a workspace node.
    pub fn is_workspace_target(&self, edge: &DependencyEdge) -> bool {
        edge.to
            .is_some_and(|t| self.get_node(t).is_some_and(|n| n.is_workspace()))
    }

    /// Mark a dependency edge as resolved.
    pub fn mark_dependency_resolved(&mut self, edge_id: EdgeIndex, target: NodeIndex) {
        if let Some(GraphEdge::Dependency(dep)) = self.graph.edge_weight_mut(edge_id) {
            dep.valid = true;
            dep.to = Some(target);
        }
    }

    /// Update the spec on a dependency edge (e.g. after resolving `catalog:` protocol).
    pub fn update_dependency_spec(&mut self, edge_id: EdgeIndex, spec: String) {
        if let Some(GraphEdge::Dependency(dep)) = self.graph.edge_weight_mut(edge_id) {
            dep.spec = spec;
        }
    }

    /// Get node by index.
    pub fn get_node(&self, index: NodeIndex) -> Option<&PackageNode> {
        self.graph.node_weight(index)
    }

    /// Get mutable node by index.
    pub fn get_node_mut(&mut self, index: NodeIndex) -> Option<&mut PackageNode> {
        self.graph.node_weight_mut(index)
    }

    /// Get all workspace nodes (excluding links).
    pub fn get_workspace_nodes(&self) -> Vec<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                self.get_node(idx)
                    .is_some_and(|node| node.is_workspace() && !node.is_link())
            })
            .collect()
    }

    /// Get all resolved dependency targets for a node.
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

    /// Serialize graph to package-lock.json format.
    ///
    /// Builds `LockPackage` structs directly — no intermediate `serde_json::Value`.
    /// Delegates to `package_lock::serialize_to_packages` for the actual implementation.
    #[inline]
    pub fn serialize_to_packages(
        &self,
        root_path: &Path,
    ) -> (HashMap<String, super::package_lock::LockPackage>, i32) {
        super::package_lock::serialize_to_packages(self, root_path)
    }

    /// Serialize to package-lock.json format, dropping any node not reachable
    /// from the importers via resolved dependency edges. Used on the
    /// lockfile-reuse path, where seeding inserts the whole prior tree and the
    /// BFS may leave some of it orphaned: a removed direct dependency's subtree,
    /// or a node shadowed by a re-resolved (bumped) version. See
    /// [`reachable_nodes`](Self::reachable_nodes).
    #[inline]
    pub fn serialize_to_packages_pruned(
        &self,
        root_path: &Path,
    ) -> (HashMap<String, super::package_lock::LockPackage>, i32) {
        super::package_lock::serialize_to_packages_filtered(
            self,
            root_path,
            Some(&self.reachable_nodes()),
        )
    }

    /// The set of nodes reachable from the importers (root + workspace members)
    /// by following **resolved** dependency edges. This is the same traversal
    /// [`compute_node_types`](crate::resolver::node_types::compute_node_types)
    /// uses to assign node types, so "has a type" and "is reachable" agree.
    ///
    /// On a cold resolve every placed node is reachable (the resolver only
    /// creates a node when it resolves an edge to it), so this is the identity
    /// — it only prunes on the reuse path, where seeded-but-orphaned nodes
    /// remain physically attached under the append-only graph invariant.
    pub fn reachable_nodes(&self) -> HashSet<NodeIndex> {
        let mut reachable: HashSet<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|&i| {
                self.get_node(i)
                    .is_some_and(|n| n.is_root() || n.is_workspace())
            })
            .collect();
        let mut stack: Vec<NodeIndex> = reachable.iter().copied().collect();
        while let Some(node) = stack.pop() {
            // Walk resolved dependency edges by their target index only — no need
            // for `get_resolved_dependencies`, which would clone each dep name.
            let targets: Vec<NodeIndex> = self
                .graph
                .edges_directed(node, Outgoing)
                .filter_map(|edge| match edge.weight() {
                    GraphEdge::Dependency(dep) if dep.valid => dep.to,
                    _ => None,
                })
                .collect();
            for target in targets {
                if reachable.insert(target) {
                    stack.push(target);
                }
            }
        }
        reachable
    }

    /// Find compatible node in parent chain for dependency resolution.
    ///
    /// For unconditional overrides (spec == "*"), uses the override target_spec.
    /// For conditional overrides (spec != "*"), override is checked later in
    /// process_dependency using the resolved version.
    pub fn find_compatible_node(
        &self,
        from: NodeIndex,
        name: &str,
        version_spec: &str,
    ) -> FindResult {
        // Check for unconditional override (spec == "*", no resolved version yet)
        let effective_spec = if let Some(target) = self.check_override(from, name, None) {
            tracing::debug!(
                "Using unconditional override for {}@{} => {}",
                name,
                version_spec,
                target
            );
            target
        } else {
            version_spec.to_string()
        };

        // Get physical parent of from node, default to from if it's the root
        let parent = self.get_physical_parent(from).unwrap_or(from);

        // Recursively search up the parent chain
        self.find_in_parent_chain(parent, name, &effective_spec, from)
    }

    /// Recursively search for compatible node in parent chain.
    fn find_in_parent_chain(
        &self,
        current: NodeIndex,
        name: &str,
        spec: &str,
        requester: NodeIndex,
    ) -> FindResult {
        // Probe the per-parent name index — O(depth) total instead of a
        // linear scan over every physical child per ancestor level.
        if let Some(child_idx) = self.find_physical_child(current, name) {
            let child = &self.graph[child_idx];
            if matches(spec, &child.version) {
                return FindResult::Reuse(child_idx);
            }
            tracing::debug!(
                "found conflict deps {}@{} got {}, conflict at {:?}",
                name,
                spec,
                child.version,
                child_idx
            );
            return FindResult::Conflict(requester);
        }

        // Recurse to parent
        if let Some(parent) = self.get_physical_parent(current) {
            self.find_in_parent_chain(parent, name, spec, requester)
        } else {
            // Reached root, install here
            FindResult::New(current)
        }
    }
}

/// Result of finding a compatible node.
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

    fn create_pkg(name: &str, version: &str) -> PackageJson {
        PackageJson::new(name, version)
    }

    fn create_version_manifest(name: &str, version: &str) -> Arc<CoreVersionManifest> {
        Arc::new(CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        })
    }

    #[test]
    fn test_create_graph() {
        let pkg = create_pkg("test", "1.0.0");
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);
        assert_eq!(graph.graph.node_count(), 1);
        assert!(graph.graph[graph.root_index].is_root());
    }

    #[test]
    fn test_add_nodes_and_edges() {
        let pkg = create_pkg("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        let child = PackageNode::from_version_manifest(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            create_version_manifest("lodash", "4.17.21"),
        );
        let child_idx = graph.add_node(child);

        graph.add_physical_edge(graph.root_index, child_idx);

        assert_eq!(graph.graph.node_count(), 2);
        assert_eq!(graph.get_physical_children(graph.root_index).len(), 1);
        assert_eq!(graph.get_physical_parent(child_idx), Some(graph.root_index));
    }

    #[test]
    fn test_dependency_edges() {
        let pkg = create_pkg("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

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
        let child = PackageNode::from_version_manifest(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            create_version_manifest("lodash", "4.17.21"),
        );
        let child_idx = graph.add_node(child);
        graph.mark_dependency_resolved(edge_id, child_idx);

        let deps = graph.get_dependency_edges(graph.root_index);
        assert!(deps[0].1.valid);
        assert_eq!(deps[0].1.to, Some(child_idx));
    }

    #[test]
    fn test_logical_ancestry_traces_hoisted_chain() {
        // root has a dep on A; A has a dep on B; B has a dep on C.
        // All three are hoisted flat under root (physical tree: root→A, root→B, root→C).
        // Logical chain for C should be root → A → B → C, NOT just root → C.
        let pkg = create_pkg("my-app", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        let a_idx = graph.add_node(PackageNode::from_version_manifest(
            "a".to_string(),
            PathBuf::from("node_modules/a"),
            create_version_manifest("a", "1.0.0"),
        ));
        let b_idx = graph.add_node(PackageNode::from_version_manifest(
            "b".to_string(),
            PathBuf::from("node_modules/b"),
            create_version_manifest("b", "2.0.0"),
        ));
        let c_idx = graph.add_node(PackageNode::from_version_manifest(
            "c".to_string(),
            PathBuf::from("node_modules/c"),
            create_version_manifest("c", "3.0.0"),
        ));

        // Hoisted installation layout — all three are direct physical children of root.
        graph.add_physical_edge(graph.root_index, a_idx);
        graph.add_physical_edge(graph.root_index, b_idx);
        graph.add_physical_edge(graph.root_index, c_idx);

        // Logical edges: root → A, A → B, B → C
        let e1 = graph.add_dependency_edge(
            graph.root_index,
            "a".to_string(),
            "^1.0.0".to_string(),
            EdgeType::Prod,
        );
        graph.mark_dependency_resolved(e1, a_idx);
        let e2 =
            graph.add_dependency_edge(a_idx, "b".to_string(), "^2.0.0".to_string(), EdgeType::Prod);
        graph.mark_dependency_resolved(e2, b_idx);
        let e3 =
            graph.add_dependency_edge(b_idx, "c".to_string(), "^3.0.0".to_string(), EdgeType::Prod);
        graph.mark_dependency_resolved(e3, c_idx);

        let chain = graph.logical_ancestry(c_idx);
        let names: Vec<&str> = chain.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["my-app", "a", "b", "c"]);
    }

    #[test]
    fn test_find_compatible_node_reuse() {
        let pkg = create_pkg("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Add lodash@4.17.21 under root
        let lodash = PackageNode::from_version_manifest(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            create_version_manifest("lodash", "4.17.21"),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // Should reuse existing lodash when spec matches
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::Reuse(lodash_idx));
    }

    #[test]
    fn test_find_compatible_node_conflict() {
        let pkg = create_pkg("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Add lodash@3.10.1 under root
        let lodash = PackageNode::from_version_manifest(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            create_version_manifest("lodash", "3.10.1"),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // Should find conflict when spec doesn't match
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::Conflict(graph.root_index));
    }

    #[test]
    fn test_find_compatible_node_new() {
        let pkg = create_pkg("root", "1.0.0");
        let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Should return New when no existing node found
        let result = graph.find_compatible_node(graph.root_index, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::New(graph.root_index));
    }

    #[test]
    fn test_find_compatible_node_nested() {
        let pkg = create_pkg("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

        // Add express under root
        let express = PackageNode::from_version_manifest(
            "express".to_string(),
            PathBuf::from("node_modules/express"),
            create_version_manifest("express", "4.18.0"),
        );
        let express_idx = graph.add_node(express);
        graph.add_physical_edge(graph.root_index, express_idx);

        // Add lodash@4.17.21 under root
        let lodash = PackageNode::from_version_manifest(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            create_version_manifest("lodash", "4.17.21"),
        );
        let lodash_idx = graph.add_node(lodash);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // From express, should find lodash in parent (root)
        let result = graph.find_compatible_node(express_idx, "lodash", "^4.17.0");
        assert_eq!(result, FindResult::Reuse(lodash_idx));
    }
}
