use anyhow::Result;
use turbo_tasks::{CollectiblesSource, OperationVc, ResolvedVc, Vc, get_effects};
use turbopack_core::{diagnostics::Diagnostic, issue::CollectibleIssuesExt};

use crate::{
    endpoint::{Endpoint, OptionEndpoint},
    entrypoint::Entrypoints,
};

/// Based on [`Entrypoints`], but with [`OperationVc<Endpoint>`][OperationVc] for every endpoint.
///
/// This is used when constructing `ExternalEndpoint`s in the `napi` crate.
///
/// This is important as `OperationVc`s can be stored in the VersionedContentMap and can be exposed
/// to JS via napi.
///
/// This is needed to call `write_to_disk` which expects an `OperationVc<Endpoint>`.
#[turbo_tasks::value(shared)]
pub struct EntrypointsOperation {
    pub apps: OperationVc<OptionEndpoint>,
    pub libraries: OperationVc<OptionEndpoint>,
}

/// HACK: Wraps an `OperationVc<Entrypoints>` inside of a second `OperationVc`.
#[turbo_tasks::function(operation)]
fn entrypoints_wrapper(entrypoints: OperationVc<Entrypoints>) -> Vc<Entrypoints> {
    entrypoints.connect()
}

/// Removes diagnostics, issues, and effects from the top-level `entrypoints` operation so that
/// they're not duplicated across many different individual entrypoints or routes.
#[turbo_tasks::function(operation)]
async fn entrypoints_without_collectibles_operation(
    entrypoints: OperationVc<Entrypoints>,
) -> Result<Vc<Entrypoints>> {
    let _ = entrypoints.read_strongly_consistent().await?;
    entrypoints.drop_collectibles::<Box<dyn Diagnostic>>();
    entrypoints.drop_issues();
    let _ = get_effects(entrypoints).await?;
    Ok(entrypoints.connect())
}

#[turbo_tasks::value_impl]
impl EntrypointsOperation {
    #[turbo_tasks::function(operation)]
    pub async fn new(entrypoints: OperationVc<Entrypoints>) -> Result<Vc<Self>> {
        let e = entrypoints.connect().await?;
        let entrypoints = entrypoints_without_collectibles_operation(entrypoints);
        Ok(Self {
            apps: match e.apps.as_ref() {
                Some(es) => match es.await?.first().copied() {
                    Some(endpoint) => wrap_as_option_endpoint(endpoint, entrypoints),
                    None => empty_option_endpoint(),
                },
                None => empty_option_endpoint(),
            },
            libraries: match e.libraries.as_ref() {
                Some(es) => match es.await?.first().copied() {
                    Some(endpoint) => wrap_as_option_endpoint(endpoint, entrypoints),
                    None => empty_option_endpoint(),
                },
                None => empty_option_endpoint(),
            },
        }
        .cell())
    }
}

/// Wraps a resolved `Endpoint` as `OptionEndpoint(Some(...))` while keeping the `Entrypoints`
/// operation alive via `op.connect()`.
#[turbo_tasks::function(operation)]
fn wrap_as_option_endpoint(
    endpoint: ResolvedVc<Box<dyn Endpoint>>,
    op: OperationVc<Entrypoints>,
) -> Vc<OptionEndpoint> {
    let _ = op.connect();
    OptionEndpoint(Some(endpoint)).cell()
}

/// Returns an `OperationVc<OptionEndpoint>` representing the absent-endpoint case.
#[turbo_tasks::function(operation)]
fn empty_option_endpoint() -> Vc<OptionEndpoint> {
    OptionEndpoint(None).cell()
}
