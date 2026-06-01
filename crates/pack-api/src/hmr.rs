use std::sync::Arc;

use anyhow::Result;
use turbo_rcstr::RcStr;
use turbo_tasks::{Effects, ReadRef, ResolvedVc, Vc, take_effects};
use turbopack_core::{
    issue::PlainIssue,
    version::{Update, VersionState},
};

use crate::{
    project::{Project, ProjectContainer},
    utils::get_issues,
};

#[turbo_tasks::value(shared, serialization = "skip")]
pub struct HmrUpdateWithIssues {
    pub update: ReadRef<Update>,
    pub issues: Arc<Vec<ReadRef<PlainIssue>>>,
    pub effects: Arc<Effects>,
}

#[turbo_tasks::function(operation, root)]
pub async fn hmr_update_with_issues_operation(
    project: ResolvedVc<Project>,
    identifier: RcStr,
    state: ResolvedVc<VersionState>,
) -> Result<Vc<HmrUpdateWithIssues>> {
    let update_op = project_hmr_update_operation(project, identifier, state);
    let update = update_op.read_strongly_consistent().await?;
    let issues = get_issues(update_op).await?;
    let effects = Arc::new(take_effects(update_op).await?);
    Ok(HmrUpdateWithIssues {
        update,
        issues,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation, root)]
fn project_hmr_update_operation(
    project: ResolvedVc<Project>,
    identifier: RcStr,
    state: ResolvedVc<VersionState>,
) -> Vc<Update> {
    project.hmr_update(identifier, *state)
}

#[turbo_tasks::value(shared, serialization = "skip")]
pub struct HmrIdentifiersWithIssues {
    pub identifiers: ReadRef<Vec<RcStr>>,
    pub issues: Arc<Vec<ReadRef<PlainIssue>>>,
    pub effects: Arc<Effects>,
}

#[turbo_tasks::function(operation, root)]
pub async fn get_hmr_identifiers_with_issues_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<HmrIdentifiersWithIssues>> {
    let hmr_identifiers_op = project_container_hmr_identifiers_operation(container);
    let hmr_identifiers = hmr_identifiers_op.read_strongly_consistent().await?;
    let issues = get_issues(hmr_identifiers_op).await?;
    let effects = Arc::new(take_effects(hmr_identifiers_op).await?);
    Ok(HmrIdentifiersWithIssues {
        identifiers: hmr_identifiers,
        issues,
        effects,
    }
    .cell())
}

#[turbo_tasks::function(operation, root)]
fn project_container_hmr_identifiers_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Vc<Vec<RcStr>> {
    container.hmr_identifiers()
}
