//! Dependency edge management for the dependency graph.

use std::collections::HashMap;

use petgraph::graph::{EdgeIndex, NodeIndex};

use crate::model::graph::DependencyGraph;
use crate::model::manifest::CoreVersionManifest;
use crate::model::node::{DevDeps, EdgeType, PeerDeps};
use crate::model::package_json::PackageJson;
use crate::spec::{Catalogs, resolve_catalog_spec};

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
        let mut deps: Vec<_> = deps.iter().collect();
        deps.sort_unstable_by_key(|(name, _)| *name);
        for (name, spec) in deps {
            f(edge_type, name, spec);
        }
    }
}

/// Trait for types that can provide dependency information.
pub trait DependencySource {
    fn for_each_dep<F>(&self, peer_deps: PeerDeps, dev_deps: DevDeps, callback: F)
    where
        F: FnMut(EdgeType, &str, &str);
}

impl DependencySource for PackageJson {
    fn for_each_dep<F>(&self, peer_deps: PeerDeps, dev_deps: DevDeps, mut f: F)
    where
        F: FnMut(EdgeType, &str, &str),
    {
        iter_deps(self.dependencies.as_ref(), EdgeType::Prod, &mut f);
        if dev_deps == DevDeps::Include {
            iter_deps(self.dev_dependencies.as_ref(), EdgeType::Dev, &mut f);
        }
        if peer_deps == PeerDeps::Include {
            iter_deps(self.peer_dependencies.as_ref(), EdgeType::Peer, &mut f);
        }
        iter_deps(
            self.optional_dependencies.as_ref(),
            EdgeType::Optional,
            &mut f,
        );
    }
}

impl DependencySource for CoreVersionManifest {
    fn for_each_dep<F>(&self, peer_deps: PeerDeps, dev_deps: DevDeps, mut f: F)
    where
        F: FnMut(EdgeType, &str, &str),
    {
        // npm registry may copy optionalDependencies into dependencies (legacy bug)
        // Skip deps that also appear in optionalDependencies to avoid duplicate edges
        let optional_deps = self.optional_dependencies.as_ref();
        iter_deps(
            self.dependencies.as_ref(),
            EdgeType::Prod,
            &mut |_, name, spec| {
                if !optional_deps.is_some_and(|opt| opt.contains_key(name)) {
                    f(EdgeType::Prod, name, spec);
                }
            },
        );
        if dev_deps == DevDeps::Include {
            iter_deps(self.dev_dependencies.as_ref(), EdgeType::Dev, &mut f);
        }
        if peer_deps == PeerDeps::Include {
            iter_deps(self.peer_dependencies.as_ref(), EdgeType::Peer, &mut f);
        }
        iter_deps(optional_deps, EdgeType::Optional, &mut f);
    }
}

/// Context for creating dependency edges.
pub struct EdgeContext<'a> {
    pub peer_deps: PeerDeps,
    pub dev_deps: DevDeps,
    pub catalogs: Option<&'a Catalogs>,
}

impl<'a> EdgeContext<'a> {
    pub fn new(peer_deps: PeerDeps, dev_deps: DevDeps) -> Self {
        Self {
            peer_deps,
            dev_deps,
            catalogs: None,
        }
    }

    pub fn with_catalogs(mut self, catalogs: &'a Catalogs) -> Self {
        self.catalogs = Some(catalogs);
        self
    }
}

/// Add dependency edges from any source that implements `DependencySource`.
///
/// `catalog:` specs are resolved to their concrete version range at edge
/// creation time (like `workspace:` specs), so downstream code sees only
/// registry-compatible specs.
pub fn add_edges_from<S: DependencySource>(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    source: &S,
    ctx: &EdgeContext<'_>,
) {
    source.for_each_dep(ctx.peer_deps, ctx.dev_deps, |edge_type, name, spec| {
        let resolved = ctx
            .catalogs
            .and_then(|c| resolve_catalog_spec(name, spec, c))
            .unwrap_or(spec);
        graph.add_dependency_edge(node_index, name, resolved, edge_type);
    });
}
