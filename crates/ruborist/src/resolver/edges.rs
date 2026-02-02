//! Dependency edge management for the dependency graph.

use std::collections::HashMap;

use petgraph::graph::{EdgeIndex, NodeIndex};

use crate::model::graph::DependencyGraph;
use crate::model::manifest::VersionManifest;
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;

/// Represents an unresolved dependency edge extracted from the graph.
#[derive(Debug, Clone)]
pub struct DependencyEdgeInfo {
    pub edge_id: EdgeIndex,
    pub name: String,
    pub spec: String,
    pub edge_type: EdgeType,
}

/// Collect only unresolved dependency edges from a node.
pub fn collect_unresolved_edges(
    graph: &DependencyGraph,
    node_index: NodeIndex,
) -> Vec<DependencyEdgeInfo> {
    graph
        .get_dependency_edges(node_index)
        .into_iter()
        .filter(|(_, dep)| !dep.valid)
        .map(|(edge_id, dep)| DependencyEdgeInfo {
            edge_id,
            name: dep.name.clone(),
            spec: dep.spec.clone(),
            edge_type: dep.edge_type,
        })
        .collect()
}

#[inline]
fn iter_deps<F>(deps: Option<&HashMap<String, String>>, edge_type: EdgeType, f: &mut F)
where
    F: FnMut(EdgeType, &str, &str),
{
    if let Some(deps) = deps {
        for (name, spec) in deps {
            f(edge_type, name, spec);
        }
    }
}

/// Trait for types that can provide dependency information.
pub trait DependencySource {
    fn for_each_dep<F>(&self, legacy_peer_deps: bool, include_dev: bool, callback: F)
    where
        F: FnMut(EdgeType, &str, &str);
}

impl DependencySource for PackageJson {
    fn for_each_dep<F>(&self, legacy_peer_deps: bool, include_dev: bool, mut f: F)
    where
        F: FnMut(EdgeType, &str, &str),
    {
        iter_deps(Some(&self.dependencies), EdgeType::Prod, &mut f);
        if include_dev {
            iter_deps(Some(&self.dev_dependencies), EdgeType::Dev, &mut f);
        }
        if !legacy_peer_deps {
            iter_deps(Some(&self.peer_dependencies), EdgeType::Peer, &mut f);
        }
        iter_deps(
            Some(&self.optional_dependencies),
            EdgeType::Optional,
            &mut f,
        );
    }
}

impl DependencySource for VersionManifest {
    fn for_each_dep<F>(&self, legacy_peer_deps: bool, include_dev: bool, mut f: F)
    where
        F: FnMut(EdgeType, &str, &str),
    {
        // npm registry may copy optionalDependencies into dependencies (legacy bug)
        // Skip deps that also appear in optionalDependencies to avoid duplicate edges
        let optional_deps = self.optional_dependencies.as_ref();
        for (name, spec) in self.dependencies.as_ref().into_iter().flatten() {
            if !optional_deps.is_some_and(|opt| opt.contains_key(name)) {
                f(EdgeType::Prod, name, spec);
            }
        }
        if include_dev {
            iter_deps(self.dev_dependencies.as_ref(), EdgeType::Dev, &mut f);
        }
        if !legacy_peer_deps {
            iter_deps(self.peer_dependencies.as_ref(), EdgeType::Peer, &mut f);
        }
        iter_deps(optional_deps, EdgeType::Optional, &mut f);
    }
}

/// Add dependency edges from any source that implements `DependencySource`.
pub fn add_edges_from<S: DependencySource>(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    source: &S,
    legacy_peer_deps: bool,
    include_dev: bool,
) {
    source.for_each_dep(legacy_peer_deps, include_dev, |edge_type, name, spec| {
        graph.add_dependency_edge(node_index, name, spec, edge_type);
    });
}
