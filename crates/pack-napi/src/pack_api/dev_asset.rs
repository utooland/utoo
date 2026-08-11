use std::sync::Arc;

use anyhow::Result;
use futures_util::TryFutureExt;
use hyper::{HeaderMap, Uri};
use napi::{
    JsFunction,
    bindgen_prelude::{Buffer, External},
};
use pack_api::{
    endpoint::Endpoint,
    entrypoint::{EntrypointsWithIssues, get_entrypoints_with_issues_operation},
    project::ProjectContainer,
    utils::{get_issues, strongly_consistent_catch_collectables},
};
use turbo_rcstr::RcStr;
use turbo_tasks::{
    CollectiblesSource, Completion, Effects, OperationVc, ReadRef, ResolvedVc, TransientInstance,
    Vc, read_strongly_consistent_and_apply_effects, take_effects, trace::TraceRawVcs,
};
use turbo_tasks_fs::{FileContent, FileSystemPath};
use turbopack_core::{
    asset::{Asset, AssetContent},
    issue::PlainIssue,
    output::{OutputAsset, OutputAssets, OutputAssetsReference, OutputAssetsWithReferenced},
    version::{
        NotFoundVersion, PartialUpdate, TotalUpdate, Update, Version, VersionState,
        VersionedContent,
    },
};
use turbopack_dev_server::source::{
    Body, ContentSource, ContentSourceSideEffect, HeaderList,
    asset_graph::AssetGraphContentSource,
    combined::CombinedContentSource,
    request::SourceRequest,
    resolve::{ResolveSourceRequestResult, resolve_source_request},
};
use turbopack_ecmascript_hmr_protocol::{ClientUpdateInstruction, ResourceIdentifier};

use super::{
    project::{NapiEntrypoints, ProjectInstance, collect_endpoint_output_paths},
    turbopack_ctx::{RootTask, TurbopackContext},
    utils::{NapiIssue, TurbopackResult, subscribe},
};
use crate::util::DetachedVc;

pub struct DevAssetSourceInstance {
    source: DetachedVc<Box<dyn ContentSource>>,
}

const DEV_ASSET_GRAPH_ROOT: &str = "__utoo_dev_asset_graph_root__";

#[turbo_tasks::value]
struct DevAssetGraphRoot {
    path: FileSystemPath,
    references: ResolvedVc<OutputAssets>,
}

#[turbo_tasks::value_impl]
impl OutputAssetsReference for DevAssetGraphRoot {
    #[turbo_tasks::function]
    fn references(&self) -> Vc<OutputAssetsWithReferenced> {
        OutputAssetsWithReferenced::from_assets(*self.references)
    }
}

#[turbo_tasks::value_impl]
impl OutputAsset for DevAssetGraphRoot {
    #[turbo_tasks::function]
    fn path(&self) -> Vc<FileSystemPath> {
        self.path.clone().cell()
    }
}

#[turbo_tasks::value_impl]
impl Asset for DevAssetGraphRoot {
    #[turbo_tasks::function]
    fn content(&self) -> Vc<AssetContent> {
        AssetContent::File(FileContent::NotFound.resolved_cell()).cell()
    }
}

impl DevAssetSourceInstance {
    pub fn operation_for_container(
        container: ResolvedVc<ProjectContainer>,
    ) -> OperationVc<Box<dyn ContentSource>> {
        project_dev_asset_source_operation(container)
    }

    pub fn new(
        turbopack_ctx: TurbopackContext,
        source: OperationVc<Box<dyn ContentSource>>,
    ) -> Self {
        Self {
            source: DetachedVc::new(turbopack_ctx, source),
        }
    }

    fn operation(&self) -> OperationVc<Box<dyn ContentSource>> {
        *self.source
    }
}

#[turbo_tasks::function(operation, root)]
async fn project_dev_asset_source_operation(
    container: ResolvedVc<ProjectContainer>,
) -> Result<Vc<Box<dyn ContentSource>>> {
    let project = container.project().to_resolved().await?;

    if let Some(app_project) = *project.app_project().await? {
        let endpoint = app_project.get_app_endpoint();
        let output = endpoint.output().await?;
        let client_root = project.client_root().owned().await?;
        let root = DevAssetGraphRoot {
            path: client_root.join(DEV_ASSET_GRAPH_ROOT)?,
            references: output.output_assets,
        }
        .cell();

        return Ok(Vc::upcast(AssetGraphContentSource::new_lazy(
            client_root,
            Vc::upcast(root),
        )));
    }

    Ok(Vc::upcast(CombinedContentSource::new(Vec::new())))
}

fn source_request(path: &str) -> Result<SourceRequest> {
    let path = path.trim_start_matches('/');
    Ok(SourceRequest {
        method: "GET".to_string(),
        uri: Uri::try_from(format!("/{path}"))?,
        headers: HeaderMap::new(),
        body: Body::new(vec![]),
    })
}

#[turbo_tasks::value(shared, serialization = "skip")]
struct DevAssetResolveWithCollectibles {
    result: Option<ReadRef<DevAssetMaterializeResult>>,
    issues: Arc<Vec<ReadRef<PlainIssue>>>,
    effects: Arc<Effects>,
    side_effects: Vec<ResolvedVc<Box<dyn ContentSourceSideEffect>>>,
}

#[turbo_tasks::value(serialization = "skip")]
enum DevAssetMaterializeResult {
    Static {
        content: ReadRef<FileContent>,
        status_code: u16,
        headers: ReadRef<HeaderList>,
        header_overwrites: ReadRef<HeaderList>,
    },
    NotFound,
}

#[turbo_tasks::function(operation, root)]
async fn materialize_dev_asset_operation(
    source: OperationVc<Box<dyn ContentSource>>,
    request: TransientInstance<SourceRequest>,
) -> Result<Vc<DevAssetMaterializeResult>> {
    Ok(
        match &*resolve_source_request(source, request).connect().await? {
            ResolveSourceRequestResult::Static(static_content, header_overwrites) => {
                let static_content = static_content.await?;
                if let AssetContent::File(file) = &*static_content.content.content().await? {
                    DevAssetMaterializeResult::Static {
                        content: file.await?,
                        status_code: static_content.status_code,
                        headers: static_content.headers.await?,
                        header_overwrites: header_overwrites.await?,
                    }
                } else {
                    DevAssetMaterializeResult::NotFound
                }
            }
            _ => DevAssetMaterializeResult::NotFound,
        }
        .cell(),
    )
}

#[turbo_tasks::function(operation, root)]
async fn resolve_dev_asset_with_collectibles_operation(
    source: OperationVc<Box<dyn ContentSource>>,
    request: TransientInstance<SourceRequest>,
) -> Result<Vc<DevAssetResolveWithCollectibles>> {
    let materialize_op = materialize_dev_asset_operation(source, request);
    let (result, issues, effects) = strongly_consistent_catch_collectables(materialize_op).await?;
    let side_effects = materialize_op.peek_collectibles().into_iter().collect();

    Ok(DevAssetResolveWithCollectibles {
        result,
        issues,
        effects,
        side_effects,
    }
    .cell())
}

#[turbo_tasks::function(operation, root)]
async fn apply_side_effects_operation(
    side_effects: Vec<ResolvedVc<Box<dyn ContentSourceSideEffect>>>,
) -> Result<Vc<Completion>> {
    for side_effect in side_effects {
        side_effect.apply().await?;
    }
    Ok(Completion::new())
}

async fn resolve_dev_asset(
    source: OperationVc<Box<dyn ContentSource>>,
    path: RcStr,
) -> Result<ReadRef<DevAssetResolveWithCollectibles>> {
    let request = TransientInstance::new(source_request(path.as_str())?);
    let op = resolve_dev_asset_with_collectibles_operation(source, request);
    let resolved = read_strongly_consistent_and_apply_effects(op, |value| &value.effects).await?;

    if !resolved.side_effects.is_empty() {
        apply_side_effects_operation(resolved.side_effects.clone())
            .read_strongly_consistent()
            .await?;
    }

    Ok(resolved)
}

#[napi(object)]
pub struct NapiDevAssetHeader {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct NapiDevAsset {
    pub status_code: u32,
    pub headers: Vec<NapiDevAssetHeader>,
    pub body: Buffer,
}

#[napi(object)]
pub struct NapiDevAssetResponse {
    pub asset: Option<NapiDevAsset>,
    pub issues: Vec<NapiIssue>,
}

#[derive(TraceRawVcs)]
struct DevAssetData {
    status_code: u32,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some(header) = headers
        .iter_mut()
        .find(|header| header.0.eq_ignore_ascii_case(name))
    {
        header.1 = value;
    } else {
        headers.push((name.to_string(), value));
    }
}

fn to_dev_asset_data(
    result: &DevAssetMaterializeResult,
    request_path: &str,
) -> Result<Option<DevAssetData>> {
    let DevAssetMaterializeResult::Static {
        content,
        status_code,
        headers: static_headers,
        header_overwrites,
    } = result
    else {
        return Ok(None);
    };
    let FileContent::Content(file) = &**content else {
        return Ok(None);
    };

    let mut body = Vec::with_capacity(file.content().len());
    for chunk in file.content().read() {
        body.extend_from_slice(chunk);
    }

    let mut headers = Vec::new();
    for (name, value) in static_headers.iter() {
        headers.push((name.to_string(), value.to_string()));
    }
    for (name, value) in header_overwrites.iter() {
        set_header(&mut headers, name, value.to_string());
    }
    let content_type = file
        .content_type()
        .cloned()
        .unwrap_or_else(|| mime_guess::from_path(request_path).first_or_octet_stream());
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        set_header(&mut headers, "content-type", content_type.to_string());
    }
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cache-control"))
    {
        set_header(&mut headers, "cache-control", "must-revalidate".to_string());
    }
    set_header(&mut headers, "content-length", body.len().to_string());

    Ok(Some(DevAssetData {
        status_code: (*status_code).into(),
        headers,
        body,
    }))
}

#[napi]
pub async fn project_get_dev_asset(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    path: String,
) -> napi::Result<NapiDevAssetResponse> {
    let ctx = &project.turbopack_ctx;
    let source = project.dev_asset_source.operation();
    let request_path = path.clone();

    let (asset, issues) = ctx
        .turbo_tasks()
        .run(async move {
            let resolved = resolve_dev_asset(source, path.into()).await?;
            let asset = if let Some(result) = &resolved.result {
                to_dev_asset_data(result, &request_path)?
            } else {
                None
            };
            Ok((asset, resolved.issues.clone()))
        })
        .or_else(|error| ctx.throw_turbopack_internal_result(&error.into()))
        .await?;

    Ok(NapiDevAssetResponse {
        asset: asset.map(|asset| NapiDevAsset {
            status_code: asset.status_code,
            headers: asset
                .headers
                .into_iter()
                .map(|(name, value)| NapiDevAssetHeader { name, value })
                .collect(),
            body: asset.body.into(),
        }),
        issues: issues
            .iter()
            .map(|issue| NapiIssue::from(&**issue))
            .collect(),
    })
}

#[napi]
pub async fn project_prepare_dev_assets(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
) -> napi::Result<TurbopackResult<NapiEntrypoints>> {
    let ctx = &project.turbopack_ctx;
    let container = project.container;
    let source = project.dev_asset_source.operation();
    let tt = ctx.turbo_tasks();

    let (entrypoints, app_paths, library_paths, issues) = tt
        .run(async move {
            let entrypoints_op = get_entrypoints_with_issues_operation(container);
            let entrypoints =
                read_strongly_consistent_and_apply_effects(entrypoints_op, |value| &value.effects)
                    .await?;
            let EntrypointsWithIssues {
                entrypoints,
                issues,
                effects: _,
            } = &*entrypoints;
            let app_paths = collect_endpoint_output_paths(&entrypoints.apps).await?;
            let library_paths = collect_endpoint_output_paths(&entrypoints.libraries).await?;
            let root = resolve_dev_asset(source, DEV_ASSET_GRAPH_ROOT.into()).await?;
            let mut all_issues = issues.iter().cloned().collect::<Vec<_>>();
            for issue in root.issues.iter() {
                if !all_issues.contains(issue) {
                    all_issues.push(issue.clone());
                }
            }
            Ok((entrypoints.clone(), app_paths, library_paths, all_issues))
        })
        .or_else(|error| ctx.throw_turbopack_internal_result(&error.into()))
        .await?;

    let mut result = NapiEntrypoints::from_entrypoints_op(&entrypoints, ctx)?;
    result.app_paths = Some(
        app_paths
            .into_iter()
            .map(|path| super::endpoint::NapiWrittenEndpoint::from(Some(path)))
            .collect(),
    );
    result.library_paths = Some(
        library_paths
            .into_iter()
            .map(|path| super::endpoint::NapiWrittenEndpoint::from(Some(path)))
            .collect(),
    );

    Ok(TurbopackResult {
        result,
        issues: issues
            .iter()
            .map(|issue| NapiIssue::from(&**issue))
            .collect(),
    })
}

#[turbo_tasks::function(operation, root)]
async fn resolve_dev_asset_operation(
    source: OperationVc<Box<dyn ContentSource>>,
    request: TransientInstance<SourceRequest>,
) -> Result<Vc<ResolveSourceRequestResult>> {
    Ok(resolve_source_request(source, request).connect())
}

#[turbo_tasks::function(operation, root)]
async fn initial_dev_asset_version_operation(
    source: OperationVc<Box<dyn ContentSource>>,
    request: TransientInstance<SourceRequest>,
) -> Result<Vc<Box<dyn Version>>> {
    let result = resolve_dev_asset_operation(source, request)
        .read_strongly_consistent()
        .await?;
    Ok(match &*result {
        ResolveSourceRequestResult::Static(static_content, _) => {
            let static_content = static_content.await?;
            if static_content.status_code == 404 {
                Vc::upcast(NotFoundVersion::new())
            } else {
                static_content.content.version()
            }
        }
        _ => Vc::upcast(NotFoundVersion::new()),
    })
}

#[turbo_tasks::function]
async fn dev_asset_version_state(
    source: OperationVc<Box<dyn ContentSource>>,
    path: RcStr,
    session: TransientInstance<()>,
) -> Result<Vc<VersionState>> {
    let _ = session;
    let request = TransientInstance::new(source_request(path.as_str())?);
    let version = initial_dev_asset_version_operation(source, request)
        .read_trait_strongly_consistent()
        .untracked()
        .await?;
    VersionState::new(version).await
}

#[turbo_tasks::function(operation, root)]
async fn dev_asset_update_operation(
    source: OperationVc<Box<dyn ContentSource>>,
    request: TransientInstance<SourceRequest>,
    state: ResolvedVc<VersionState>,
) -> Result<Vc<Update>> {
    let result = resolve_dev_asset_operation(source, request)
        .read_strongly_consistent()
        .await?;
    Ok(match &*result {
        ResolveSourceRequestResult::Static(static_content, _) => {
            let static_content = static_content.await?;
            if static_content.status_code == 404 {
                Update::Missing.cell()
            } else {
                static_content.content.update(state.get())
            }
        }
        _ => Update::Missing.cell(),
    })
}

#[turbo_tasks::value(shared, serialization = "skip")]
struct DevAssetUpdateWithIssues {
    update: ReadRef<Update>,
    issues: Arc<Vec<ReadRef<turbopack_core::issue::PlainIssue>>>,
    effects: Arc<Effects>,
}

#[turbo_tasks::function(operation, root)]
async fn dev_asset_update_with_issues_operation(
    source: OperationVc<Box<dyn ContentSource>>,
    request: TransientInstance<SourceRequest>,
    state: ResolvedVc<VersionState>,
) -> Result<Vc<DevAssetUpdateWithIssues>> {
    let update_op = dev_asset_update_operation(source, request, state);
    let update = update_op.read_strongly_consistent().await?;
    let issues = get_issues(update_op).await?;
    let effects = Arc::new(take_effects(update_op).await?);
    Ok(DevAssetUpdateWithIssues {
        update,
        issues,
        effects,
    }
    .cell())
}

#[napi(ts_return_type = "{ __napiType: \"RootTask\" }")]
pub fn project_dev_asset_hmr_events(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    identifier: RcStr,
    func: JsFunction,
) -> napi::Result<External<RootTask>> {
    let turbopack_ctx = project.turbopack_ctx.clone();
    let source = project.dev_asset_source.operation();
    let session = TransientInstance::new(());

    subscribe(
        turbopack_ctx,
        func,
        {
            let outer_identifier = identifier.clone();
            let session = session.clone();
            move || {
                let identifier = outer_identifier.clone();
                let session = session.clone();
                async move {
                    let state = dev_asset_version_state(source, identifier.clone(), session)
                        .to_resolved()
                        .await?;
                    let request = TransientInstance::new(source_request(identifier.as_str())?);
                    let update_op = dev_asset_update_with_issues_operation(source, request, state);
                    let update = read_strongly_consistent_and_apply_effects(update_op, |value| {
                        &value.effects
                    })
                    .await?;

                    match &*update.update {
                        Update::Missing | Update::None => {}
                        Update::Total(TotalUpdate { to })
                        | Update::Partial(PartialUpdate { to, .. }) => {
                            state.set(to.clone()).await?;
                        }
                    }
                    Ok((Some(update.update.clone()), update.issues.clone()))
                }
            }
        },
        move |ctx| {
            let (update, issues) = ctx.value;
            let napi_issues = issues
                .iter()
                .map(|issue| NapiIssue::from(&**issue))
                .collect();
            let update_issues = issues
                .iter()
                .map(|issue| (&**issue).into())
                .collect::<Vec<_>>();
            let resource = ResourceIdentifier {
                path: identifier.clone(),
                headers: None,
            };
            let update = match update.as_deref() {
                None | Some(Update::Missing) | Some(Update::Total(_)) => {
                    ClientUpdateInstruction::restart(&resource, &update_issues)
                }
                Some(Update::Partial(update)) => {
                    ClientUpdateInstruction::partial(&resource, &update.instruction, &update_issues)
                }
                Some(Update::None) => ClientUpdateInstruction::issues(&resource, &update_issues),
            };

            Ok(vec![TurbopackResult {
                result: ctx.env.to_js_value(&update)?,
                issues: napi_issues,
            }])
        },
    )
}
