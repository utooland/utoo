use std::{path::Path, sync::Arc};

use anyhow::{Result, ensure};
use bincode::{Decode, Encode};
use pack_core::config::Platform;
use serde::Serialize;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{
    Effects, FxIndexSet, NonLocalValue, ReadRef, ResolvedVc, TryJoinIterExt, ValueToString,
    ValueToStringRef, Vc,
};
use turbo_tasks_fs::{DiskFileSystem, File, FileContent, FileSystem};
use turbopack_core::{issue::PlainIssue, module::Module};

use super::{
    AnalyzeChunkGroup, AnalyzeRouteEntry, RouteBundleSummary, analyze_module_graphs,
    analyze_output_assets, analyze_route_entry_id,
};
use crate::{project::ProjectContainer, utils::strongly_consistent_catch_collectables};

#[derive(Clone, Debug, PartialEq, Eq, NonLocalValue, Encode, Decode)]
pub struct AnalyzeEntry {
    pub name: RcStr,
    pub path: RcStr,
}

#[turbo_tasks::value(transparent)]
struct AnalyzeEntries(Vec<AnalyzeEntry>);

#[turbo_tasks::value(serialization = "skip")]
pub struct WriteAnalyzeResult {
    pub entries: Vec<AnalyzeEntry>,
    pub issues: Arc<Vec<ReadRef<PlainIssue>>>,
    pub effects: Arc<Effects>,
}

#[derive(Serialize)]
struct RouteSummary {
    route: RcStr,
    #[serde(flatten)]
    bundles: RouteBundleSummary,
}

#[turbo_tasks::function(operation, root)]
pub async fn write_analyze_data_with_issues_operation(
    container: ResolvedVc<ProjectContainer>,
    output_path: RcStr,
) -> Result<Vc<WriteAnalyzeResult>> {
    let operation = write_analyze_data_operation(container, output_path);
    let (entries, issues, effects) = strongly_consistent_catch_collectables(operation).await?;
    Ok(WriteAnalyzeResult {
        entries: entries.map_or_else(Vec::new, |entries| entries.to_vec()),
        issues,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation, root)]
async fn write_analyze_data_operation(
    container: ResolvedVc<ProjectContainer>,
    output_path: RcStr,
) -> Result<Vc<AnalyzeEntries>> {
    ensure!(
        Path::new(output_path.as_str()).is_absolute(),
        "The bundle analysis output path must be absolute"
    );

    let project = container.project();
    ensure!(
        project.mode().await?.is_production() && !*project.is_watch_enabled().await?,
        "Bundle analysis requires a production project with watch disabled"
    );
    ensure!(
        matches!(&*project.platform().await?, Platform::Web)
            && project.library_project().await?.is_none(),
        "Bundle analysis supports browser applications, not Node.js or library builds"
    );
    let config = project.config();
    let server = config.server().await?;
    ensure!(
        !server
            .entry
            .as_ref()
            .is_some_and(|entry| entry.has_entries())
            && server.function.is_none(),
        "Bundle analysis does not yet support configured server entries or functions"
    );
    ensure!(
        *config.source_maps().await?,
        "Bundle analysis requires source maps for source-level size attribution"
    );

    let project = project.with_analyze_config();
    let app = (*project.app_project().await?)
        .ok_or_else(|| anyhow::anyhow!("Bundle analysis requires at least one browser entry"))?;
    let graph = *project.whole_app_module_graphs().await?.full;
    let entries = app.resolved_entrypoints().await?;
    let context = Vc::upcast(app.app_module_context());
    let runtime_entries = app.app_runtime_entries();
    let root = DiskFileSystem::new(rcstr!("pack-bundle-analysis"), Vc::cell(output_path))
        .root()
        .owned()
        .await?;

    let captures = entries
        .iter()
        .copied()
        .enumerate()
        .map(async |(index, entry)| {
            let name = entry.await?.name.clone();
            // Use a controlled directory name; a configured entry name is a display label.
            let slug = format!("entry-{index}");
            let route: RcStr = format!("/{slug}").into();
            let entry_modules = entry
                .entry_evaluatable_assets(context, runtime_entries)
                .await?;
            let modules = entry_modules
                .iter()
                .copied()
                .map(ResolvedVc::upcast::<Box<dyn Module>>)
                .collect::<FxIndexSet<_>>();
            let route_entries = modules
                .iter()
                .map(async |module| {
                    let ident = module.ident();
                    let module_ident = ident.to_string().owned().await?;
                    let module_path = ident.await?.path.to_string_ref().await?;
                    Ok(AnalyzeRouteEntry {
                        route_entry_id: analyze_route_entry_id(
                            &route,
                            "primary",
                            "",
                            &module_ident,
                        ),
                        module_ident,
                        module_path,
                        role: rcstr!("primary"),
                        entry_kind: Some(rcstr!("client_bootstrap")),
                        client_references: vec![],
                    })
                })
                .try_join()
                .await?;

            let output_assets = entry.output_assets_for_entry(context, runtime_entries);
            let chunk_groups = Vc::cell(vec![AnalyzeChunkGroup {
                kind: rcstr!("bootstrap"),
                trigger: modules.first().copied(),
                // This is the evaluated group's direct assets. The analyzer follows the
                // transitive output references separately to discover asynchronous groups.
                assets: output_assets.to_resolved().await?,
            }]);
            let capture =
                analyze_output_assets(output_assets, Vc::cell(route_entries), chunk_groups, graph)
                    .await?;
            root.join(&slug)?
                .join("analyze.data")?
                .write(*capture.content)
                .as_side_effect()
                .await?;
            Ok((
                AnalyzeEntry {
                    name,
                    path: route.clone(),
                },
                RouteSummary {
                    route,
                    bundles: capture.summary,
                },
            ))
        })
        .try_join()
        .await?;

    let (entries, summaries): (Vec<_>, Vec<_>) = captures.into_iter().unzip();
    root.join("modules.data")?
        .write(analyze_module_graphs(graph))
        .as_side_effect()
        .await?;
    root.join("route-summaries.json")?
        .write(FileContent::Content(File::from(serde_json::to_string(&summaries)?)).cell())
        .as_side_effect()
        .await?;
    Ok(Vc::cell(entries))
}
