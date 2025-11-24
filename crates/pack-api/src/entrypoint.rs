use anyhow::Result;
use std::sync::Arc;
use turbo_tasks::{Effects, FxIndexSet, ReadRef, ResolvedVc, TryJoinIterExt, Vc, get_effects};
use turbopack_core::{
    diagnostics::PlainDiagnostic,
    issue::PlainIssue,
    output::{OutputAsset, OutputAssets},
};

use crate::{
    endpoint::{Endpoint, Endpoints},
    operation::EntrypointsOperation,
    project::ProjectContainer,
    utils::{get_diagnostics, get_issues},
};

#[turbo_tasks::value(shared)]
pub struct Entrypoints {
    pub apps: Option<ResolvedVc<Endpoints>>,
    pub libraries: Option<ResolvedVc<Endpoints>>,
}

#[turbo_tasks::function(operation)]
pub async fn get_all_written_entrypoints_with_issues_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<EntrypointsWithIssues>> {
    let entrypoints_operation =
        EntrypointsOperation::new(all_entrypoints_write_to_disk_operation(container));
    let entrypoints = entrypoints_operation.read_strongly_consistent().await?;
    let issues = get_issues(entrypoints_operation).await?;
    let diagnostics = get_diagnostics(entrypoints_operation).await?;
    let effects = Arc::new(get_effects(entrypoints_operation).await?);
    Ok(EntrypointsWithIssues {
        entrypoints,
        issues,
        diagnostics,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation)]
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

#[turbo_tasks::function(operation)]
pub async fn all_output_assets_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<OutputAssets>> {
    let project = container.project();

    // Get assets from all endpoints
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

    Ok(Vc::cell(output_assets.into_iter().collect()))
}

#[turbo_tasks::value(shared, serialization = "none")]
pub struct EntrypointsWithIssues {
    pub entrypoints: ReadRef<EntrypointsOperation>,
    pub issues: Arc<Vec<ReadRef<PlainIssue>>>,
    pub diagnostics: Arc<Vec<ReadRef<PlainDiagnostic>>>,
    pub effects: Arc<Effects>,
}

#[turbo_tasks::function(operation)]
pub async fn get_entrypoints_with_issues_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<EntrypointsWithIssues>> {
    let entrypoints_operation =
        EntrypointsOperation::new(project_container_entrypoints_operation(container));
    let entrypoints = entrypoints_operation.read_strongly_consistent().await?;
    let issues = get_issues(entrypoints_operation).await?;
    let diagnostics = get_diagnostics(entrypoints_operation).await?;
    let effects = Arc::new(get_effects(entrypoints_operation).await?);
    Ok(EntrypointsWithIssues {
        entrypoints,
        issues,
        diagnostics,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation)]
fn project_container_entrypoints_operation(
    // the container is a long-lived object with internally mutable state, there's no risk of it
    // becoming stale
    container: ResolvedVc<ProjectContainer>,
) -> Vc<Entrypoints> {
    container.entrypoints()
}
