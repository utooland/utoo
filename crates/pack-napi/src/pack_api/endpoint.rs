use std::ops::Deref;

use super::utils::{NapiIssue, TurbopackResult, subscribe};
use crate::pack_api::turbopack_ctx::RootTask;
use crate::util::DetachedVc;
use futures_util::TryFutureExt;
use napi::{JsFunction, bindgen_prelude::External};
use pack_api::{
    endpoint::{
        EndpointIssuesAndDiags, EndpointOutputPaths, OptionEndpoint, WrittenEndpointWithIssues,
        get_written_endpoint_with_issues_operation,
    },
    paths::ServerPath,
    utils::{endpoint_client_changed_operation, subscribe_issues_and_diags_operation},
};
use tracing::Instrument;
use turbo_tasks::{ReadRef, read_strongly_consistent_and_apply_effects};

#[napi(object)]
#[derive(Default)]
pub struct NapiEndpointConfig {}

#[napi(object)]
#[derive(Default)]
pub struct NapiServerPath {
    pub path: String,
    pub content_hash: String,
}

impl From<ServerPath> for NapiServerPath {
    fn from(server_path: ServerPath) -> Self {
        Self {
            path: server_path.path,
            content_hash: server_path.content_hash.into_owned(),
        }
    }
}

#[napi(object)]
#[derive(Default)]
pub struct NapiWrittenEndpoint {
    pub r#type: String,
    pub entry_path: Option<String>,
    pub client_paths: Vec<String>,
    pub server_paths: Vec<NapiServerPath>,
    pub config: NapiEndpointConfig,
}

impl From<Option<EndpointOutputPaths>> for NapiWrittenEndpoint {
    fn from(written_endpoint: Option<EndpointOutputPaths>) -> Self {
        match written_endpoint {
            Some(EndpointOutputPaths::NodeJs {
                server_entry_path,
                server_paths,
                client_paths,
            }) => Self {
                r#type: "nodejs".to_string(),
                entry_path: Some(server_entry_path.into_owned()),
                client_paths: client_paths.into_iter().map(From::from).collect(),
                server_paths: server_paths.into_iter().map(From::from).collect(),
                ..Default::default()
            },
            Some(EndpointOutputPaths::Edge {
                server_paths,
                client_paths,
            }) => Self {
                r#type: "edge".to_string(),
                client_paths: client_paths.into_iter().map(From::from).collect(),
                server_paths: server_paths.into_iter().map(From::from).collect(),
                ..Default::default()
            },
            Some(EndpointOutputPaths::NotFound) | None => Self {
                r#type: "none".to_string(),
                ..Default::default()
            },
        }
    }
}

// NOTE(alexkirsz) We go through an extra layer of indirection here because of
// two factors:
// 1. rustc currently has a bug where using a dyn trait as a type argument to
//    some async functions (in this case `endpoint_write_to_disk`) can cause
//    higher-ranked lifetime errors. See https://github.com/rust-lang/rust/issues/102211
// 2. the type_complexity clippy lint.
pub struct ExternalEndpoint(pub DetachedVc<OptionEndpoint>);

impl Deref for ExternalEndpoint {
    type Target = DetachedVc<OptionEndpoint>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[napi]
#[tracing::instrument(skip_all)]
pub async fn endpoint_write_to_disk(
    #[napi(ts_arg_type = "{ __napiType: \"Endpoint\" }")] endpoint: External<ExternalEndpoint>,
) -> napi::Result<TurbopackResult<NapiWrittenEndpoint>> {
    let ctx = endpoint.turbopack_ctx();
    let endpoint_op = ***endpoint;
    let (written, issues) = endpoint
        .turbopack_ctx()
        .turbo_tasks()
        .run(async move {
            let written_entrypoint_with_issues_op =
                get_written_endpoint_with_issues_operation(endpoint_op);
            let written_entrypoint_with_issues = read_strongly_consistent_and_apply_effects(
                written_entrypoint_with_issues_op,
                |v| &v.effects,
            )
            .await?;
            let WrittenEndpointWithIssues {
                written,
                issues,
                effects: _,
            } = &*written_entrypoint_with_issues;

            Ok((written.clone(), issues.clone()))
        })
        .or_else(|e| ctx.throw_turbopack_internal_result(&e.into()))
        .await?;
    Ok(TurbopackResult {
        result: NapiWrittenEndpoint::from(written.map(ReadRef::into_owned)),
        issues: issues.iter().map(|i| NapiIssue::from(&**i)).collect(),
    })
}

#[napi(ts_return_type = "{ __napiType: \"RootTask\" }")]
pub fn endpoint_server_changed_subscribe(
    #[napi(ts_arg_type = "{ __napiType: \"Endpoint\" }")] endpoint: External<ExternalEndpoint>,
    issues: bool,
    func: JsFunction,
) -> napi::Result<External<RootTask>> {
    let turbopack_ctx = endpoint.turbopack_ctx().clone();
    let endpoint = ***endpoint;
    subscribe(
        turbopack_ctx,
        func,
        move || {
            async move {
                let issues_and_diags_op = subscribe_issues_and_diags_operation(endpoint, issues);
                read_strongly_consistent_and_apply_effects(issues_and_diags_op, |v| &v.effects)
                    .await
            }
            .instrument(tracing::info_span!("server changes subscription"))
        },
        |ctx| {
            let EndpointIssuesAndDiags {
                changed: _,
                issues,
                effects: _,
            } = &*ctx.value;

            Ok(vec![TurbopackResult {
                result: (),
                issues: issues.iter().map(|i| NapiIssue::from(&**i)).collect(),
            }])
        },
    )
}

#[napi(ts_return_type = "{ __napiType: \"RootTask\" }")]
pub fn endpoint_client_changed_subscribe(
    #[napi(ts_arg_type = "{ __napiType: \"Endpoint\" }")] endpoint: External<ExternalEndpoint>,
    func: JsFunction,
) -> napi::Result<External<RootTask>> {
    let turbopack_ctx = endpoint.turbopack_ctx().clone();
    let endpoint_op = ***endpoint;
    subscribe(
        turbopack_ctx,
        func,
        move || {
            async move {
                let changed_op = endpoint_client_changed_operation(endpoint_op);
                // We don't capture issues here since we don't want to be notified when they
                // change.
                //
                // This must be a *read*, not just a resolve, because we need the root task created
                // by `subscribe` to re-run when the `Completion`'s value changes (via equality),
                // even if the cell id doesn't change.
                let _ = changed_op.read_strongly_consistent().await?;
                Ok(())
            }
            .instrument(tracing::trace_span!("client changes subscription"))
        },
        |_| {
            Ok(vec![TurbopackResult {
                result: (),
                issues: vec![],
            }])
        },
    )
}
