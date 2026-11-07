use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, ensure};
use bincode::{Decode, Encode};
use pack_core::config::Platform;
use serde::Serialize;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{
    Effects, FxIndexSet, NonLocalValue, ReadRef, ResolvedVc, TryJoinIterExt, ValueToString,
    ValueToStringRef, Vc,
};
use turbo_tasks_fs::{DiskFileSystem, File, FileContent, FileSystem};
use turbopack_core::{
    issue::PlainIssue, module::Module, module_graph::ModuleGraph, output::OutputAssets,
};

use super::{
    AnalyzeChunkGroup, AnalyzeModuleGraphs, AnalyzeRouteEntry, RouteBundleSummary,
    analyze_module_graphs, analyze_module_index, analyze_output_assets, analyze_route_entry_id,
    trace::trace_files,
};
use crate::{
    app::AppProject, project::ProjectContainer, utils::strongly_consistent_catch_collectables,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, NonLocalValue, Encode, Decode)]
pub enum AnalyzeEnvironment {
    Client,
    Server,
}

impl AnalyzeEnvironment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }

    fn entry_kind(self) -> RcStr {
        match self {
            Self::Client => rcstr!("client_bootstrap"),
            Self::Server => rcstr!("server"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, NonLocalValue, Encode, Decode)]
pub struct AnalyzeEntry {
    pub name: RcStr,
    pub path: RcStr,
    pub environment: AnalyzeEnvironment,
}

struct EntryCapture {
    name: RcStr,
    environment: AnalyzeEnvironment,
    modules: Vec<ResolvedVc<Box<dyn Module>>>,
    graph: ResolvedVc<ModuleGraph>,
    assets: ResolvedVc<OutputAssets>,
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
        project.library_project().await?.is_none(),
        "Bundle analysis supports application entries, not library builds"
    );
    let config = project.config();
    let server = config.server().await?;
    ensure!(
        *config.source_maps().await?,
        "Bundle analysis requires source maps for source-level size attribution"
    );

    let project = project.with_analyze_config();
    let app = *project.app_project().await?;
    let has_server_entries = server
        .entry
        .as_ref()
        .is_some_and(|entry| entry.has_entries());
    ensure!(
        app.is_some() || has_server_entries,
        "Bundle analysis requires at least one application or server entry"
    );
    let app_context = match app {
        Some(app) => *app,
        // The server builder is also usable without browser or Node app entrypoints.
        None => AppProject::new(project, Vc::cell(vec![])),
    };
    let (app_capture, server_build) = futures::future::try_join(
        async {
            if app.is_none() {
                return Ok::<_, anyhow::Error>((None, vec![]));
            }
            let environment = match &*project.platform().await? {
                Platform::Web => AnalyzeEnvironment::Client,
                Platform::Node => AnalyzeEnvironment::Server,
            };
            let graph = project.whole_app_module_graphs().await?.full;
            let context = Vc::upcast(app_context.app_module_context());
            let runtime_entries = app_context.app_runtime_entries();
            let entries = app_context
                .resolved_entrypoints()
                .await?
                .iter()
                .copied()
                .map(async |entry| {
                    Ok(EntryCapture {
                        name: entry.await?.name.clone(),
                        environment,
                        graph,
                        modules: entry
                            .entry_evaluatable_assets(context, runtime_entries)
                            .await?
                            .iter()
                            .copied()
                            .map(ResolvedVc::upcast::<Box<dyn Module>>)
                            .collect(),
                        assets: entry
                            .output_assets_for_entry(context, runtime_entries)
                            .to_resolved()
                            .await?,
                    })
                })
                .try_join()
                .await?;
            Ok((Some(graph), entries))
        },
        async {
            if has_server_entries || server.function.is_some() {
                Ok(Some(app_context.server_build().await?))
            } else {
                Ok(None)
            }
        },
    )
    .await?;
    let (app_graph, mut entries) = app_capture;
    let mut graphs = app_graph.into_iter().collect::<Vec<_>>();
    if let Some(server_build) = server_build
        && let Some(graph) = server_build.graph
    {
        graphs.push(graph);
        entries.extend(server_build.entries.iter().map(|entry| EntryCapture {
            name: entry.name.clone(),
            environment: AnalyzeEnvironment::Server,
            modules: entry.modules.clone(),
            graph,
            assets: entry.assets,
        }));
    }
    let graphs = AnalyzeModuleGraphs(graphs).cell();
    let module_index = analyze_module_index(graphs);
    let root = DiskFileSystem::new(rcstr!("pack-bundle-analysis"), Vc::cell(output_path))
        .root()
        .owned()
        .await?;
    let project_path = project.project_path().owned().await?;

    let captures = entries
        .into_iter()
        .enumerate()
        .map(
            async |(
                index,
                EntryCapture {
                    name,
                    environment,
                    modules,
                    graph,
                    assets,
                },
            )| {
                // Use a controlled directory name; a configured entry name is a display label.
                let slug = format!("entry-{index}");
                let route: RcStr = format!("/{slug}").into();
                let modules = modules.into_iter().collect::<FxIndexSet<_>>();
                let route_entries = modules
                    .iter()
                    .map(async |module| {
                        let ident = module.ident();
                        let module_ident = ident.to_string().owned().await?;
                        let module_path = ident.await?.path.to_string_ref().await?;
                        ensure!(
                            module_index.await?.by_ident.contains_key(&module_ident),
                            "Entry module {module_ident} is missing from the analyzer module index"
                        );
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
                            entry_kind: Some(environment.entry_kind()),
                            client_references: vec![],
                        })
                    })
                    .try_join()
                    .await?;

                let chunk_groups = match environment {
                    AnalyzeEnvironment::Client => vec![AnalyzeChunkGroup {
                        kind: rcstr!("bootstrap"),
                        trigger: modules.first().copied(),
                        // The analyzer follows transitive output references separately for async groups.
                        assets,
                    }],
                    AnalyzeEnvironment::Server => vec![],
                };
                let traced_files = match environment {
                    AnalyzeEnvironment::Client => Vc::cell(vec![]),
                    AnalyzeEnvironment::Server => trace_files(
                        *graph,
                        Vc::cell(modules.into_iter().collect()),
                        project_path.clone(),
                    ),
                };
                let capture = analyze_output_assets(
                    *assets,
                    traced_files,
                    Vc::cell(route_entries),
                    Vc::cell(chunk_groups),
                    module_index,
                )
                .await
                .with_context(|| format!("Analyzing entry {name}"))?;
                root.join(&slug)?
                    .join("analyze.data")?
                    .write(*capture.content)
                    .as_side_effect()
                    .await?;
                Ok((
                    AnalyzeEntry {
                        name,
                        path: route.clone(),
                        environment,
                    },
                    RouteSummary {
                        route,
                        bundles: capture.summary,
                    },
                ))
            },
        )
        .try_join()
        .await?;

    let (entries, summaries): (Vec<_>, Vec<_>) = captures.into_iter().unzip();
    root.join("modules.data")?
        .write(analyze_module_graphs(graphs))
        .as_side_effect()
        .await?;
    root.join("route-summaries.json")?
        .write(FileContent::Content(File::from(serde_json::to_string(&summaries)?)).cell())
        .as_side_effect()
        .await?;
    Ok(Vc::cell(entries))
}
