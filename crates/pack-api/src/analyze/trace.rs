//! Follow Turbopack's traced references from the exact server entry roots.

use anyhow::Result;
use turbo_tasks::{FxIndexSet, TryFlatJoinIterExt, TryJoinIterExt, Vc};
use turbo_tasks_fs::{FileSystemEntryType, FileSystemPath};
use turbopack_core::{
    chunk::{ChunkingType, TracedMode},
    module::{Module, Modules},
    module_graph::{GraphTraversalAction, ModuleGraph, ModuleGraphSnapshot},
};

use super::AnalyzeGraphModule;

#[derive(Clone, Copy)]
pub(super) enum EdgeKind {
    Sync,
    Async,
    Traced,
}

/// A traced entry starts a separate reachability walk. Keep the two visits independent so a
/// module reached through ordinary imports first can still contribute traced dependencies.
pub(super) fn traverse_edges(
    graph: &ModuleGraphSnapshot,
    roots: impl IntoIterator<Item = AnalyzeGraphModule>,
    mut visit: impl FnMut(AnalyzeGraphModule, AnalyzeGraphModule, EdgeKind) -> Result<()>,
) -> Result<()> {
    let mut traced_roots = FxIndexSet::default();
    graph.traverse_edges_dfs(
        roots,
        &mut (),
        |parent, target, _| {
            let Some((parent, reference)) = parent else {
                return Ok(GraphTraversalAction::Continue);
            };
            let kind = match reference.chunking_type {
                ChunkingType::Traced { mode } => {
                    if mode == TracedMode::Entry {
                        visit(parent, target, EdgeKind::Traced)?;
                        traced_roots.insert(target);
                    }
                    return Ok(GraphTraversalAction::Exclude);
                }
                ChunkingType::Async => EdgeKind::Async,
                _ => EdgeKind::Sync,
            };
            visit(parent, target, kind)?;
            Ok(GraphTraversalAction::Continue)
        },
        |_, _, _| Ok(()),
        true,
        false,
    )?;
    if !traced_roots.is_empty() {
        graph.traverse_edges_dfs(
            traced_roots,
            &mut (),
            |parent, target, _| {
                if let Some((parent, _)) = parent {
                    visit(parent, target, EdgeKind::Traced)?;
                }
                Ok(GraphTraversalAction::Continue)
            },
            |_, _, _| Ok(()),
            true,
            false,
        )?;
    }
    Ok(())
}

#[turbo_tasks::value(transparent)]
pub(super) struct TraceFiles(Vec<FileSystemPath>);

#[turbo_tasks::function]
pub(super) async fn trace_files(
    graph: Vc<ModuleGraph>,
    roots: Vc<Modules>,
    project_path: FileSystemPath,
) -> Result<Vc<TraceFiles>> {
    let graph = graph.await?;
    let mut traced = FxIndexSet::default();
    traverse_edges(&graph, roots.await?.iter().copied(), |_, target, kind| {
        if matches!(kind, EdgeKind::Traced) {
            traced.insert(target);
        }
        Ok(())
    })?;
    let paths = traced
        .into_iter()
        .map(async |module| Ok(module.ident().await?.path.clone()))
        .try_join()
        .await?
        .into_iter()
        .filter(|path| path.fs == project_path.fs)
        .collect::<FxIndexSet<_>>();
    // The resolver already follows symlinks and reports the links as affecting sources. Count
    // physical payload files once, rather than counting a link's destination for every alias.
    let files = paths
        .into_iter()
        .map(async |path| {
            Ok(matches!(*path.get_type().await?, FileSystemEntryType::File).then_some(path))
        })
        .try_flat_join()
        .await?;
    Ok(Vc::cell(files))
}
