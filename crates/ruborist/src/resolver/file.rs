//! `file:` dependency resolution — directory links and local tarballs.
//!
//! The non-registry sibling of [`super::git`] and [`super::http`]: a `file:`
//! dir becomes a Link node inline; a `file:` tarball has its manifest parsed
//! in BFS (no global cache) and rejoins the normal BFS placement flow. The
//! tarball content is extracted directly into `node_modules` at install time.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use petgraph::graph::NodeIndex;

use super::builder::ProcessResult;
use super::edges::DependencyEdgeInfo;
use super::tar::read_local_tarball_manifest;
use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::manifest::NodeManifest;
use crate::model::node::EdgeType;
use crate::resolver::registry::ResolveError;
use crate::traits::registry::ResolvedPackage;

/// Handle a `file:` dep: dir → Link node inline (returns
/// `ControlFlow::Break`); tarball → read the local bytes, parse the manifest
/// via [`read_local_tarball_manifest`] (no global cache), and hand the
/// `ResolvedPackage` back to the normal BFS flow via `ControlFlow::Continue`.
/// The tarball content is materialized into `node_modules` at install time.
#[cfg(feature = "http-tarball")]
pub(crate) async fn process_file_dep<E>(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    conflict_parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    path_spec: &str,
) -> Result<std::ops::ControlFlow<ProcessResult, ResolvedPackage>, ResolveError<E>> {
    let file_err = |source: anyhow::Error| ResolveError::File {
        spec: edge.spec.clone(),
        source,
    };

    // Base dir is the on-disk source for root/workspace/link nodes, or
    // the parent of the `file:<abs>` tarball URL stamped on a transitive
    // file-tarball dep's manifest. Registry nodes have no valid base.
    let node = graph.get_node(node_index);
    let base = node
        .filter(|n| n.is_root() || n.is_workspace() || n.is_link())
        .map(|n| n.path.clone())
        .or_else(|| {
            let NodeManifest::Registry(m) = &node?.manifest else {
                return None;
            };
            let url = m.dist.tarball.as_deref()?.strip_prefix("file:")?;
            std::path::Path::new(url).parent().map(Path::to_path_buf)
        })
        .ok_or_else(|| ResolveError::Unsupported {
            spec: edge.spec.clone(),
            reason: "transitive file: deps inside a published registry package are not supported",
        })?;
    let abs = base.join(path_spec);

    let meta = match std::fs::metadata(&abs) {
        Ok(m) => m,
        Err(_) if edge.edge_type == EdgeType::Optional => {
            return Ok(ControlFlow::Break(ProcessResult::Skipped));
        }
        Err(e) => {
            return Err(file_err(
                anyhow::Error::new(e).context(format!("file: target {}", abs.display())),
            ));
        }
    };

    if meta.is_dir() {
        // Symlink install — same graph shape as a workspace link. We
        // intentionally do not walk the linked package's transitive deps
        // (npm-link semantics: the linked dir owns its own node_modules).
        let pkg = crate::model::util::read_package_json(&abs)
            .await
            .map_err(file_err)?;
        let idx = graph.add_node(PackageNode::link_from_package_json(abs, pkg));
        graph.add_physical_edge(conflict_parent, idx);
        graph.mark_dependency_resolved(edge.edge_id, idx);
        return Ok(ControlFlow::Break(ProcessResult::Created(idx)));
    }

    let manifest = match read_local_tarball_manifest(abs).await {
        Ok(m) => m,
        Err(_) if edge.edge_type == EdgeType::Optional => {
            return Ok(ControlFlow::Break(ProcessResult::Skipped));
        }
        Err(source) => return Err(file_err(source)),
    };
    Ok(ControlFlow::Continue(ResolvedPackage::from_manifest(
        manifest.name.clone(),
        Arc::new(manifest),
    )))
}
