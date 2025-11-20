use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Prod,     // Production dependency
    Dev,      // Development dependency
    Peer,     // Peer dependency
    Optional, // Optional dependency
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Root,
    Regular,
    Workspace,
    Link,
}

#[derive(Debug)]
pub struct Node {
    // Basic info (immutable)
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub package: Value,

    // Nested relationships (need mutable access)
    pub parent: RwLock<Option<Arc<Node>>>,
    pub children: RwLock<Vec<Arc<Node>>>,

    // Edge relationships (need mutable access)
    pub edges_out: RwLock<Vec<Arc<Edge>>>,
    pub edges_in: RwLock<Vec<Arc<Edge>>>,

    // Node type and flags (immutable)
    pub node_type: NodeType,
    pub target: RwLock<Option<Arc<Node>>>,

    // Dependency type flags (mutable)
    pub is_optional: RwLock<Option<bool>>,
    pub is_peer: RwLock<Option<bool>>,
    pub is_dev: RwLock<Option<bool>>,
    pub is_prod: RwLock<Option<bool>>,

    // Overrides configuration
    pub overrides: Option<super::override_rule::Overrides>,
}

#[derive(Debug)]
pub struct Edge {
    // Basic info (immutable)
    pub name: String,
    pub spec: String,

    // Relationship info (immutable)
    pub from: Arc<Node>,
    pub to: RwLock<Option<Arc<Node>>>,

    // Resolution status
    pub valid: RwLock<bool>,

    // Edge type (immutable)
    pub edge_type: EdgeType,
}

impl Node {
    pub fn new(name: String, path: PathBuf, pkg: Value) -> Arc<Self> {
        Arc::new(Self {
            name,
            version: pkg["version"].as_str().unwrap_or("").to_string(),
            path,
            package: pkg,
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            edges_out: RwLock::new(Vec::new()),
            edges_in: RwLock::new(Vec::new()),
            node_type: NodeType::Regular,
            target: RwLock::new(None),
            is_dev: RwLock::new(None),
            is_peer: RwLock::new(None),
            is_optional: RwLock::new(None),
            is_prod: RwLock::new(None),
            overrides: None,
        })
    }

    pub fn new_root(name: String, path: PathBuf, pkg: Value) -> Arc<Self> {
        Arc::new(Self {
            name,
            version: pkg["version"].as_str().unwrap_or("").to_string(),
            path,
            package: pkg.clone(),
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            edges_out: RwLock::new(Vec::new()),
            edges_in: RwLock::new(Vec::new()),
            node_type: NodeType::Root,
            target: RwLock::new(None),
            is_dev: RwLock::new(None),
            is_peer: RwLock::new(None),
            is_optional: RwLock::new(None),
            is_prod: RwLock::new(None),
            overrides: super::override_rule::Overrides::parse(pkg),
        })
    }

    pub fn new_link(name: String, target: Arc<Node>) -> Arc<Self> {
        Arc::new(Self {
            name,
            path: target.path.clone(),
            package: target.package.clone(),
            version: target.version.clone(),
            target: RwLock::new(Some(target)),
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            edges_out: RwLock::new(Vec::new()),
            edges_in: RwLock::new(Vec::new()),
            node_type: NodeType::Link,
            is_dev: RwLock::new(None),
            is_peer: RwLock::new(None),
            is_optional: RwLock::new(None),
            is_prod: RwLock::new(None),
            overrides: None,
        })
    }

    pub fn new_workspace(name: String, path: PathBuf, pkg: Value) -> Arc<Self> {
        Arc::new(Self {
            name,
            version: pkg["version"].as_str().unwrap_or("*").to_string(),
            path,
            package: pkg,
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            edges_out: RwLock::new(Vec::new()),
            edges_in: RwLock::new(Vec::new()),
            node_type: NodeType::Workspace,
            target: RwLock::new(None),
            is_dev: RwLock::new(None),
            is_peer: RwLock::new(None),
            is_optional: RwLock::new(None),
            is_prod: RwLock::new(None),
            overrides: None,
        })
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

    // Add incoming edge reference
    pub fn add_invoke(&self, edge: &Arc<Edge>) {
        let mut edges = self.edges_in.write().unwrap();
        edges.push(edge.clone());
    }

    pub async fn add_edge(&self, mut edge: Arc<Edge>) {
        // Find root node for override rules
        let mut current = Some(edge.from.clone());
        let mut root = None;

        while let Some(node) = current {
            if node.is_root() {
                root = Some(node);
                break;
            }
            current = node.parent.read().unwrap().as_ref().cloned();
        }

        // Apply override rules if exists
        if let Some(root) = root
            && let Some(overrides) = &root.overrides
        {
            // Collect parent chain information
            let mut parent_chain = Vec::new();
            let mut current_node = edge.from.parent.read().unwrap().clone();

            while let Some(node) = current_node {
                parent_chain.push((node.name.clone(), node.version.clone()));
                current_node = node.parent.read().unwrap().clone();
            }

            // Check each rule
            for rule in &overrides.rules {
                if overrides
                    .matches_rule(rule, &edge.name, &edge.spec, &parent_chain)
                    .await
                {
                    if let Some(edge_mut) = Arc::get_mut(&mut edge) {
                        tracing::debug!(
                            "Override rule applied {}@{} => {}",
                            rule.name,
                            rule.spec,
                            rule.target_spec
                        );
                        edge_mut.spec = rule.target_spec.clone();
                    }
                    break;
                }
            }
        }

        let mut edges = self.edges_out.write().unwrap();
        edges.push(edge);
    }

    // Update node type based on incoming edges
    pub fn update_type(&self) {
        if self.is_root() {
            return;
        }

        let edges_in = self.edges_in.read().unwrap();
        if edges_in.is_empty() {
            return;
        }

        let mut has_prod = false;
        let mut has_dev = false;
        let mut has_optional = false;
        let mut has_peer = false;
        let mut all_dev_or_optional = true;

        // Analyze incoming edges
        for edge in edges_in.iter() {
            let from_node = &edge.from;
            let from_is_prod = *from_node.is_prod.read().unwrap() == Some(true);
            let from_is_dev = *from_node.is_dev.read().unwrap() == Some(true);
            let from_is_optional = *from_node.is_optional.read().unwrap() == Some(true);
            let from_is_peer = *from_node.is_peer.read().unwrap() == Some(true);

            // Check if this edge brings prod dependency
            if from_is_prod && edge.edge_type == EdgeType::Prod {
                has_prod = true;
                break;
            }

            // Track dev edges
            if from_is_dev || edge.edge_type == EdgeType::Dev {
                has_dev = true;
            }

            // Track optional edges
            if from_is_optional || edge.edge_type == EdgeType::Optional {
                has_optional = true;
            }

            // Track peer edges
            if from_is_peer || edge.edge_type == EdgeType::Peer {
                has_peer = true;
            }

            // Check if edge is neither dev nor optional
            if !from_is_dev
                && edge.edge_type != EdgeType::Dev
                && !from_is_optional
                && edge.edge_type != EdgeType::Optional
            {
                all_dev_or_optional = false;
            }
        }

        // Update node status
        let mut changed = false;

        if has_prod {
            if *self.is_prod.read().unwrap() != Some(true) {
                *self.is_prod.write().unwrap() = Some(true);
                *self.is_optional.write().unwrap() = Some(false);
                *self.is_dev.write().unwrap() = Some(false);
                *self.is_peer.write().unwrap() = Some(false);
                changed = true;
            }
        } else if has_dev && has_optional && all_dev_or_optional {
            // devOptional: has both dev and optional edges, and no prod edges
            let curr_dev = *self.is_dev.read().unwrap();
            let curr_optional = *self.is_optional.read().unwrap();
            if curr_dev != Some(true) || curr_optional != Some(true) {
                *self.is_dev.write().unwrap() = Some(true);
                *self.is_optional.write().unwrap() = Some(true);
                *self.is_prod.write().unwrap() = Some(false);
                changed = true;
            }
        } else if has_optional && !has_dev && !has_peer {
            if *self.is_optional.read().unwrap() != Some(true) {
                *self.is_optional.write().unwrap() = Some(true);
                *self.is_prod.write().unwrap() = Some(false);
                changed = true;
            }
        } else if has_dev && !has_optional && !has_peer {
            if *self.is_dev.read().unwrap() != Some(true) {
                *self.is_dev.write().unwrap() = Some(true);
                *self.is_prod.write().unwrap() = Some(false);
                changed = true;
            }
        } else if has_peer && *self.is_peer.read().unwrap() != Some(true) {
            *self.is_peer.write().unwrap() = Some(true);
            *self.is_prod.write().unwrap() = Some(false);
            changed = true;
        }

        // Propagate changes
        if changed {
            tracing::debug!(
                "{}@{} type changed [dev:{} optional:{} peer:{}]",
                &self.name,
                &self.version,
                has_dev,
                has_optional,
                has_peer
            );

            let edges_out = self.edges_out.read().unwrap();
            for edge in edges_out.iter() {
                if let Some(to_node) = edge.to.read().unwrap().as_ref() {
                    to_node.update_type();
                }
            }
        }
    }
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.version)?;

        if !self.is_root()
            && let Some(parent) = self.parent.read().unwrap().as_ref()
        {
            write!(f, " <- {parent}")?;
        }

        Ok(())
    }
}

impl Edge {
    pub fn new(from: Arc<Node>, edge_type: EdgeType, name: String, spec: String) -> Arc<Self> {
        Arc::new(Self {
            name,
            spec: if spec.trim().is_empty() {
                "*".to_string()
            } else {
                spec
            },
            from,
            to: RwLock::new(None),
            valid: RwLock::new(false),
            edge_type,
        })
    }
}
