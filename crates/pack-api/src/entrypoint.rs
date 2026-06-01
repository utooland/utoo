use anyhow::Result;
use std::sync::Arc;
use turbo_tasks::{Effects, FxIndexSet, ReadRef, ResolvedVc, TryJoinIterExt, Vc, take_effects};
use turbo_tasks_fs::{File, FileContent, FileSystemPath};
use turbopack_core::{
    asset::AssetContent,
    issue::PlainIssue,
    output::{OutputAsset, OutputAssets},
    virtual_output::VirtualOutputAsset,
};

use crate::{
    endpoint::{Endpoint, Endpoints},
    operation::EntrypointsOperation,
    project::ProjectContainer,
    utils::get_issues,
    webpack_stats::generate_webpack_stats,
};

#[turbo_tasks::value(shared)]
pub struct Entrypoints {
    pub apps: Option<ResolvedVc<Endpoints>>,
    pub libraries: Option<ResolvedVc<Endpoints>>,
}

#[turbo_tasks::function(operation, root)]
pub async fn get_all_written_entrypoints_with_issues_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<EntrypointsWithIssues>> {
    let entrypoints_operation =
        EntrypointsOperation::new(all_entrypoints_write_to_disk_operation(container));
    let entrypoints = entrypoints_operation.read_strongly_consistent().await?;
    let issues = get_issues(entrypoints_operation).await?;
    let effects = Arc::new(take_effects(entrypoints_operation).await?);
    Ok(EntrypointsWithIssues {
        entrypoints,
        issues,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation, root)]
pub async fn all_entrypoints_write_to_disk_operation(
    project: ResolvedVc<ProjectContainer>,
) -> Result<Vc<Entrypoints>> {
    let _ = project
        .project()
        .emit_all_output_assets(all_output_assets_operation(project))
        .resolve()
        .await?;

    Ok(project.entrypoints())
}

#[turbo_tasks::function(operation, root)]
pub async fn all_output_assets_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<OutputAssets>> {
    let project = container.project();

    let endpoint_assets = project
        .get_all_endpoints()
        .await?
        .iter()
        .map(|endpoint| async move { endpoint.output().await?.output_assets.await })
        .try_join()
        .await?;

    let output_assets: FxIndexSet<ResolvedVc<Box<dyn OutputAsset>>> = endpoint_assets
        .iter()
        .flat_map(|assets| assets.iter().copied())
        .collect();

    let output_assets: Vc<OutputAssets> = Vc::cell(output_assets.into_iter().collect());

    if !*container.project().should_create_webpack_stats().await? {
        return Ok(output_assets);
    }

    let dist_root = container.project().dist_root();
    let server_config = container.project().config().server().await?;
    let has_server = server_config.entry.is_some() || server_config.function.is_some();

    let mut stats_outputs: Vec<ResolvedVc<Box<dyn OutputAsset>>> = Vec::new();

    if !has_server {
        stats_outputs.push(make_stats_output(output_assets, dist_root).await?);
    } else {
        let server_dist_root_vc = container.project().server_dist_root();
        let server_dist_root_read = server_dist_root_vc.await?;
        let mut client: Vec<ResolvedVc<Box<dyn OutputAsset>>> = Vec::new();
        let mut server: Vec<ResolvedVc<Box<dyn OutputAsset>>> = Vec::new();
        for asset in output_assets.await?.iter().copied() {
            if asset.path().await?.is_inside_ref(&server_dist_root_read) {
                server.push(asset);
            } else {
                client.push(asset);
            }
        }
        stats_outputs.push(make_stats_output(Vc::cell(client), dist_root).await?);
        if !server.is_empty() {
            stats_outputs.push(make_stats_output(Vc::cell(server), server_dist_root_vc).await?);
        }
    }

    Ok(output_assets.concatenate(*ResolvedVc::cell(stats_outputs)))
}

async fn make_stats_output(
    assets: Vc<OutputAssets>,
    dist_root: Vc<FileSystemPath>,
) -> Result<ResolvedVc<Box<dyn OutputAsset>>> {
    let webpack_stats = generate_webpack_stats(assets, dist_root).await?;
    let stats_json = serde_json::to_string_pretty(&*webpack_stats)?;
    let dist_root_owned = dist_root.owned().await?;
    let stats_output = VirtualOutputAsset::new(
        dist_root_owned.join("stats.json")?,
        AssetContent::file(FileContent::from(File::from(stats_json)).cell()),
    )
    .to_resolved()
    .await?;
    Ok(ResolvedVc::upcast(stats_output))
}

#[turbo_tasks::value(shared, serialization = "skip")]
pub struct EntrypointsWithIssues {
    pub entrypoints: ReadRef<EntrypointsOperation>,
    pub issues: Arc<Vec<ReadRef<PlainIssue>>>,
    pub effects: Arc<Effects>,
}

#[turbo_tasks::function(operation, root)]
pub async fn get_entrypoints_with_issues_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<EntrypointsWithIssues>> {
    let entrypoints_operation =
        EntrypointsOperation::new(project_container_entrypoints_operation(container));
    let entrypoints = entrypoints_operation.read_strongly_consistent().await?;
    let issues = get_issues(entrypoints_operation).await?;
    let effects = Arc::new(take_effects(entrypoints_operation).await?);
    Ok(EntrypointsWithIssues {
        entrypoints,
        issues,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation, root)]
fn project_container_entrypoints_operation(
    // the container is a long-lived object with internally mutable state, there's no risk of it
    // becoming stale
    container: ResolvedVc<ProjectContainer>,
) -> Vc<Entrypoints> {
    container.entrypoints()
}
