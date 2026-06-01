use futures_util::TryFutureExt;
use std::{future::Future, path::PathBuf, sync::Arc};

use crate::pack_api::turbopack_ctx::{RootTask, TurbopackContext};
use anyhow::{Result, anyhow};
use either::Either;
use napi::{
    JsFunction, JsObject, JsUnknown, NapiRaw, NapiValue, Status,
    bindgen_prelude::{External, ToNapiValue},
    threadsafe_function::{ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use pack_api::{turbo_tasks::UtooTurboTasks, utils::StyledStringSerialize};
use serde::Serialize;
use turbo_tasks::{
    TurboTasks, TurboTasksCallApi, Vc,
    message_queue::{CompilationEvent, Severity},
};
use turbo_tasks_backend::{
    BackendOptions, GitVersionInfo, StartupCacheState, StorageMode, TurboTasksBackend,
    db_invalidation::invalidation_reasons, noop_backing_storage, turbo_backing_storage,
};
use turbo_tasks_fs::FileContent;
use turbopack_core::{
    issue::{PlainIssue, PlainIssueSource, PlainSource},
    source_pos::SourcePos,
};

pub fn create_turbo_tasks(
    output_path: PathBuf,
    persistent_caching: bool,
    _memory_limit: usize,
    dependency_tracking: bool,
    is_short_session: bool,
) -> Result<UtooTurboTasks> {
    Ok(if persistent_caching {
        let version_info = GitVersionInfo {
            describe: env!("VERGEN_GIT_DESCRIBE"),
            dirty: option_env!("CI").is_none_or(|value| value.is_empty())
                && env!("VERGEN_GIT_DIRTY") == "true",
        };

        // TODO: check is_ci;
        let is_ci: bool = false;
        let (backing_storage, cache_state) = turbo_backing_storage(
            &output_path.join(".turbopack/.cache"),
            &version_info,
            is_ci,
            is_short_session,
            false,
        )?;
        let tt = TurboTasks::new(TurboTasksBackend::new(
            BackendOptions {
                storage_mode: Some(if std::env::var("TURBO_ENGINE_READ_ONLY").is_ok() {
                    StorageMode::ReadOnly
                } else if is_ci || is_short_session {
                    StorageMode::ReadWriteOnShutdown
                } else {
                    StorageMode::ReadWrite
                }),
                dependency_tracking,
                num_workers: Some(tokio::runtime::Handle::current().metrics().num_workers()),
                ..Default::default()
            },
            Either::Left(backing_storage),
        ));
        if let StartupCacheState::Invalidated { reason_code } = cache_state {
            tt.send_compilation_event(Arc::new(StartupCacheInvalidationEvent { reason_code }));
        }

        tt
    } else {
        TurboTasks::new(TurboTasksBackend::new(
            BackendOptions {
                storage_mode: None,
                dependency_tracking,
                ..Default::default()
            },
            Either::Right(noop_backing_storage()),
        ))
    })
}

#[derive(Serialize)]
struct StartupCacheInvalidationEvent {
    reason_code: Option<String>,
}

impl CompilationEvent for StartupCacheInvalidationEvent {
    fn type_name(&self) -> &'static str {
        "StartupCacheInvalidationEvent"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn message(&self) -> String {
        let reason_msg = match self.reason_code.as_deref() {
            Some(invalidation_reasons::PANIC) => {
                " because we previously detected an internal error in Turbopack"
            }
            Some(invalidation_reasons::USER_REQUEST) => " as the result of a user request",
            _ => "", // ignore unknown reasons
        };
        format!(
            "Turbopack's persistent cache has been deleted{reason_msg}. Builds or page loads may \
             be slower as a result."
        )
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[napi]
pub fn root_task_dispose(
    #[napi(ts_arg_type = "{ __napiType: \"RootTask\" }")] mut root_task: External<RootTask>,
) -> napi::Result<()> {
    if let Some(task) = root_task.task_id.take() {
        root_task
            .turbopack_ctx
            .turbo_tasks()
            .dispose_root_task(task);
    }
    Ok(())
}

#[napi(object)]
pub struct NapiIssue {
    pub severity: String,
    pub stage: String,
    pub file_path: String,
    pub title: serde_json::Value,
    pub description: Option<serde_json::Value>,
    pub detail: Option<serde_json::Value>,
    pub source: Option<NapiIssueSource>,
    pub documentation_link: String,
    pub import_traces: serde_json::Value,
}

impl From<&PlainIssue> for NapiIssue {
    fn from(issue: &PlainIssue) -> Self {
        Self {
            description: issue
                .description
                .as_ref()
                .map(|styled| serde_json::to_value(StyledStringSerialize::from(styled)).unwrap()),
            stage: issue.stage.to_string(),
            file_path: issue.file_path.to_string(),
            detail: issue
                .detail
                .as_ref()
                .map(|styled| serde_json::to_value(StyledStringSerialize::from(styled)).unwrap()),
            documentation_link: issue.documentation_link.to_string(),
            severity: issue.severity.as_str().to_string(),
            source: issue.source.as_ref().map(|source| source.into()),
            title: serde_json::to_value(StyledStringSerialize::from(&issue.title)).unwrap(),
            import_traces: serde_json::to_value(&issue.import_traces).unwrap(),
        }
    }
}

#[napi(object)]
pub struct NapiIssueSource {
    pub source: NapiSource,
    pub range: Option<NapiIssueSourceRange>,
}

impl From<&PlainIssueSource> for NapiIssueSource {
    fn from(
        PlainIssueSource {
            asset: source,
            range,
        }: &PlainIssueSource,
    ) -> Self {
        Self {
            source: (&**source).into(),
            range: range.as_ref().map(|range| range.into()),
        }
    }
}

#[napi(object)]
pub struct NapiIssueSourceRange {
    pub start: NapiSourcePos,
    pub end: NapiSourcePos,
}

impl From<&(SourcePos, SourcePos)> for NapiIssueSourceRange {
    fn from((start, end): &(SourcePos, SourcePos)) -> Self {
        Self {
            start: (*start).into(),
            end: (*end).into(),
        }
    }
}

#[napi(object)]
pub struct NapiSource {
    pub ident: String,
    pub content: Option<String>,
}

impl From<&PlainSource> for NapiSource {
    fn from(source: &PlainSource) -> Self {
        Self {
            ident: source.ident.to_string(),
            content: match &*source.content {
                FileContent::Content(content) => match content.content().to_str() {
                    Ok(str) => Some(str.into_owned()),
                    Err(_) => None,
                },
                FileContent::NotFound => None,
            },
        }
    }
}

#[napi(object)]
pub struct NapiSourcePos {
    pub line: u32,
    pub column: u32,
}

impl From<SourcePos> for NapiSourcePos {
    fn from(pos: SourcePos) -> Self {
        Self {
            line: pos.line,
            column: pos.column,
        }
    }
}

pub struct TurbopackResult<T: ToNapiValue> {
    pub result: T,
    pub issues: Vec<NapiIssue>,
}

impl<T: ToNapiValue> ToNapiValue for TurbopackResult<T> {
    unsafe fn to_napi_value(
        env: napi::sys::napi_env,
        val: Self,
    ) -> napi::Result<napi::sys::napi_value> {
        let mut obj = unsafe { napi::Env::from_raw(env).create_object() }?;

        let result = unsafe { T::to_napi_value(env, val.result) }?;
        let result = unsafe { JsUnknown::from_raw(env, result) }?;
        if matches!(result.get_type()?, napi::ValueType::Object) {
            // SAFETY: We know that result is an object, so we can cast it to a JsObject
            let result = unsafe { result.cast::<JsObject>() };

            for key in JsObject::keys(&result)? {
                let value: JsUnknown = result.get_named_property(&key)?;
                obj.set_named_property(&key, value)?;
            }
        }

        obj.set_named_property("issues", val.issues)?;

        Ok(unsafe { obj.raw() })
    }
}

pub fn subscribe<T: 'static + Send + Sync, F: Future<Output = Result<T>> + Send, V: ToNapiValue>(
    ctx: TurbopackContext,
    func: JsFunction,
    handler: impl 'static + Sync + Send + Clone + Fn() -> F,
    mapper: impl 'static + Sync + Send + FnMut(ThreadSafeCallContext<T>) -> napi::Result<Vec<V>>,
) -> napi::Result<External<RootTask>> {
    let func: ThreadsafeFunction<T> = func.create_threadsafe_function(0, mapper)?;
    let task_id = ctx.turbo_tasks().spawn_root_task({
        let ctx = ctx.clone();
        move || {
            let ctx = ctx.clone();
            let handler = handler.clone();
            let func = func.clone();
            async move {
                let result = handler()
                    .or_else(|e| ctx.throw_turbopack_internal_result(&e))
                    .await;

                let status = func.call(result, ThreadsafeFunctionCallMode::NonBlocking);
                if !matches!(status, Status::Ok) {
                    let error = anyhow!("Error calling JS function: {}", status);
                    eprintln!("{error}");
                    return Err::<Vc<()>, _>(error);
                }
                Ok(Default::default())
            }
        }
    });
    Ok(External::new(RootTask {
        turbopack_ctx: ctx,
        task_id: Some(task_id),
    }))
}
