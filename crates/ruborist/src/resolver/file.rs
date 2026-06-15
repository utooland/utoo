//! `file:` dependency resolution — directory links and local tarballs.
//!
//! The non-registry sibling of [`super::git`] and [`super::http`]: a `file:`
//! dir becomes a Link node inline; a `file:` tarball is committed into the
//! shared cache-slot contract and rejoins the normal BFS placement flow.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use petgraph::graph::NodeIndex;

use super::builder::ProcessResult;
use super::edges::DependencyEdgeInfo;
use super::http::file_cache_slot;
use super::tar::commit_tarball_bytes;
use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::manifest::NodeManifest;
use crate::model::node::EdgeType;
use crate::resolver::registry::ResolveError;
use crate::traits::registry::ResolvedPackage;

/// Handle a `file:` dep: dir → Link node inline (returns
/// `ControlFlow::Break`); tarball → stream bytes through the shared
/// `commit_tarball_bytes` and hand the `ResolvedPackage` back to the
/// normal BFS flow via `ControlFlow::Continue`.
#[cfg(feature = "http-tarball")]
pub(crate) async fn process_file_dep<E>(
    graph: &mut DependencyGraph,
    node_index: NodeIndex,
    conflict_parent: NodeIndex,
    edge: &DependencyEdgeInfo,
    path_spec: &str,
    cache_dir: Option<&Path>,
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

    let cache_dir = cache_dir
        .ok_or_else(|| file_err(anyhow::anyhow!("cache_dir required for file: tarball")))?
        .to_path_buf();
    let slot = file_cache_slot(&abs);
    let pinned = format!("file:{}", abs.display());
    let manifest = match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let bytes = std::fs::read(&abs)
            .with_context(|| format!("failed to read tarball {}", abs.display()))?;
        commit_tarball_bytes(&cache_dir, &bytes, pinned, &slot)
    })
    .await
    {
        Ok(Ok(m)) => m,
        Ok(Err(_)) | Err(_) if edge.edge_type == EdgeType::Optional => {
            return Ok(ControlFlow::Break(ProcessResult::Skipped));
        }
        Ok(Err(source)) => return Err(file_err(source)),
        Err(join) => return Err(file_err(join.into())),
    };
    Ok(ControlFlow::Continue(ResolvedPackage::from_manifest(
        manifest.name.clone(),
        Arc::new(manifest),
    )))
}
