use anyhow::Result;
use turbo_tasks::{OperationVc, Vc, take_effects};
use turbopack_core::issue::CollectibleIssuesExt;

use crate::{endpoint::OptionEndpoint, entrypoint::Entrypoints};

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
    pub apps: Vec<OperationVc<OptionEndpoint>>,
    pub libraries: Vec<OperationVc<OptionEndpoint>>,
}

/// HACK: Wraps an `OperationVc<Entrypoints>` inside of a second `OperationVc`.
#[turbo_tasks::function(operation)]
fn entrypoints_wrapper(entrypoints: OperationVc<Entrypoints>) -> Vc<Entrypoints> {
    entrypoints.connect()
}

/// Removes issues and effects from the top-level `entrypoints` operation so that they're not
/// duplicated across many different individual entrypoints or routes.
#[turbo_tasks::function(operation)]
async fn entrypoints_without_collectibles_operation(
    entrypoints: OperationVc<Entrypoints>,
) -> Result<Vc<Entrypoints>> {
    let _ = entrypoints.read_strongly_consistent().await?;
    entrypoints.drop_issues();
    let _ = take_effects(entrypoints).await?;
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
                Some(es) => (0..es.await?.len())
                    .map(|index| pick_app_endpoint(entrypoints, index))
                    .collect(),
                None => Vec::new(),
            },
            libraries: match e.libraries.as_ref() {
                Some(es) => (0..es.await?.len())
                    .map(|index| pick_library_endpoint(entrypoints, index))
                    .collect(),
                None => Vec::new(),
            },
        }
        .cell())
    }
}

/// Selects an app endpoint from the original [`Entrypoints`] operation.
#[turbo_tasks::function(operation)]
async fn pick_app_endpoint(
    op: OperationVc<Entrypoints>,
    index: usize,
) -> Result<Vc<OptionEndpoint>> {
    let entrypoints = op.read_strongly_consistent().await?;
    let endpoint = match entrypoints.apps.as_ref() {
        Some(endpoints) => endpoints.await?.get(index).copied(),
        None => None,
    };
    Ok(OptionEndpoint(endpoint).cell())
}

/// Selects a library endpoint from the original [`Entrypoints`] operation.
#[turbo_tasks::function(operation)]
async fn pick_library_endpoint(
    op: OperationVc<Entrypoints>,
    index: usize,
) -> Result<Vc<OptionEndpoint>> {
    let entrypoints = op.read_strongly_consistent().await?;
    let endpoint = match entrypoints.libraries.as_ref() {
        Some(endpoints) => endpoints.await?.get(index).copied(),
        None => None,
    };
    Ok(OptionEndpoint(endpoint).cell())
}
