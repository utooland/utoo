use std::sync::Arc;

use anyhow::Result;
use pack_api::{
    analyze::{AnalyzeDataOutputAsset, ModulesDataOutputAsset},
    project::ProjectContainer,
    utils::strongly_consistent_catch_collectables,
};
use turbo_tasks::{Effects, ReadRef, ResolvedVc, Vc};
use turbo_tasks_fs::{File, FileContent};
use turbopack_core::{
    asset::AssetContent,
    diagnostics::PlainDiagnostic,
    issue::PlainIssue,
    output::{OutputAsset, OutputAssets},
    virtual_output::VirtualOutputAsset,
};

#[turbo_tasks::value(serialization = "none")]
pub struct WriteAnalyzeResult {
    pub issues: Arc<Vec<ReadRef<PlainIssue>>>,
    pub diagnostics: Arc<Vec<ReadRef<PlainDiagnostic>>>,
    pub effects: Arc<Effects>,
}

#[turbo_tasks::function(operation)]
pub async fn write_analyze_data_with_issues_operation(
    project: ResolvedVc<ProjectContainer>,
) -> Result<Vc<WriteAnalyzeResult>> {
    let analyze_data_op = write_analyze_data_with_issues_operation_inner(project);

    let (_analyze_data, issues, diagnostics, effects) =
        strongly_consistent_catch_collectables(analyze_data_op).await?;

    Ok(WriteAnalyzeResult {
        issues,
        diagnostics,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation)]
async fn write_analyze_data_with_issues_operation_inner(
    project: ResolvedVc<ProjectContainer>,
) -> Result<()> {
    let analyze_data_op = get_analyze_data_operation(project);

    project
        .project()
        .emit_all_output_assets(analyze_data_op)
        .as_side_effect()
        .await?;

    Ok(())
}

#[turbo_tasks::function(operation)]
async fn get_analyze_data_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<OutputAssets>> {
    let project = container.project();
    let analyze_output_root = project
        .dist_root()
        .owned()
        .await?
        .join("diagnostics/analyze/data")?;

    let whole_app_module_graphs = project.to_resolved().await?.whole_app_module_graphs();

    let mut route_names = Vec::new();
    let mut analyze_assets = Vec::<ResolvedVc<Box<dyn OutputAsset>>>::new();

    let app_project = project.app_project().to_resolved().await?.await?;
    if let Some(app_project) = *app_project {
        let app_endpoint = app_project.get_app_endpoint().to_resolved().await?;
        let app_targets = app_endpoint.analyze_targets().await?;
        for target in app_targets.iter() {
            let route = format!("/apps/{}", target.name);
            route_names.push(route.clone());

            let analyze_data = AnalyzeDataOutputAsset::new(
                analyze_output_root
                    .join(route.trim_start_matches('/'))?
                    .join("analyze.data")?,
                *target.output_assets,
            )
            .to_resolved()
            .await?;
            analyze_assets.push(ResolvedVc::upcast(analyze_data));
        }
    }

    let library_project = project.library_project().to_resolved().await?.await?;
    if let Some(library_project) = *library_project {
        let library_endpoints = library_project.get_library_endpoints().await?;
        for endpoint in library_endpoints.iter() {
            let target = endpoint.analyze_target().await?;
            let route = format!("/libraries/{}", target.name);
            route_names.push(route.clone());

            let analyze_data = AnalyzeDataOutputAsset::new(
                analyze_output_root
                    .join(route.trim_start_matches('/'))?
                    .join("analyze.data")?,
                *target.output_assets,
            )
            .to_resolved()
            .await?;
            analyze_assets.push(ResolvedVc::upcast(analyze_data));
        }
    }

    whole_app_module_graphs.as_side_effect().await?;

    let modules_data = ResolvedVc::upcast(
        ModulesDataOutputAsset::new(
            analyze_output_root.join("modules.data")?,
            Vc::cell(vec![whole_app_module_graphs.await?.full]),
        )
        .to_resolved()
        .await?,
    );

    let routes_json = serde_json::to_string_pretty(&route_names)?;
    let routes_data = ResolvedVc::upcast(
        VirtualOutputAsset::new(
            analyze_output_root.join("routes.json")?,
            AssetContent::file(FileContent::from(File::from(routes_json)).cell()),
        )
        .to_resolved()
        .await?,
    );

    analyze_assets.push(modules_data);
    analyze_assets.push(routes_data);

    Ok(Vc::cell(analyze_assets))
}
