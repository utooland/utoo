use std::{future::Future, ops::Deref, path::PathBuf, sync::Arc};

use anyhow::{Result, anyhow};
use napi::{
    JsFunction, JsObject, JsUnknown, NapiRaw, NapiValue, Status,
    bindgen_prelude::{External, ToNapiValue},
    threadsafe_function::{ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use pack_api::{
    tasks::{BundlerTurboTasks, RootTask},
    utils::StyledStringSerialize,
};
use rustc_hash::FxHashMap;
use serde::Serialize;
use turbo_tasks::{
    OperationVc, TurboTasks, TurboTasksApi, Vc,
    message_queue::{CompilationEvent, Severity},
};
use turbo_tasks_backend::{
    GitVersionInfo, StartupCacheState, db_invalidation::invalidation_reasons,
    default_backing_storage, noop_backing_storage,
};
use turbo_tasks_fs::FileContent;
use turbopack_core::{
    diagnostics::PlainDiagnostic,
    error::PrettyPrintError,
    issue::{PlainIssue, PlainIssueSource, PlainSource, StyledString},
    source_pos::SourcePos,
};

use crate::util::log_internal_error_and_inform;

pub fn create_turbo_tasks(
    output_path: PathBuf,
    persistent_caching: bool,
    _memory_limit: usize,
    dependency_tracking: bool,
) -> Result<BundlerTurboTasks> {
    Ok(if persistent_caching {
        let version_info = GitVersionInfo {
            describe: env!("VERGEN_GIT_DESCRIBE"),
            dirty: option_env!("CI").is_none_or(|value| value.is_empty())
                && env!("VERGEN_GIT_DIRTY") == "true",
        };

        // TODO: check is_ci;
        let is_ci: bool = false;
        let (backing_storage, cache_state) =
            default_backing_storage(&output_path.join(".turbopack/.cache"), &version_info, is_ci)?;
        let tt = TurboTasks::new(turbo_tasks_backend::TurboTasksBackend::new(
            turbo_tasks_backend::BackendOptions {
                storage_mode: Some(if std::env::var("TURBO_ENGINE_READ_ONLY").is_ok() {
                    turbo_tasks_backend::StorageMode::ReadOnly
                } else {
                    turbo_tasks_backend::StorageMode::ReadWrite
                }),
                dependency_tracking,
                ..Default::default()
            },
            backing_storage,
        ));
        if let StartupCacheState::Invalidated { reason_code } = cache_state {
            tt.send_compilation_event(Arc::new(StartupCacheInvalidationEvent { reason_code }));
        }

        BundlerTurboTasks::PersistentCaching(tt)
    } else {
        BundlerTurboTasks::Memory(TurboTasks::new(
            turbo_tasks_backend::TurboTasksBackend::new(
                turbo_tasks_backend::BackendOptions {
                    storage_mode: None,
                    dependency_tracking,
                    ..Default::default()
                },
                noop_backing_storage(),
            ),
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

/// A helper type to hold both a Vc operation and the TurboTasks root process.
/// Without this, we'd need to pass both individually all over the place
#[derive(Clone)]
pub struct VcArc<T> {
    turbo_tasks: BundlerTurboTasks,
    /// The Vc. Must be unresolved, otherwise you are referencing an inactive operation.
    vc: OperationVc<T>,
}

impl<T> VcArc<T> {
    pub fn new(turbo_tasks: BundlerTurboTasks, vc: OperationVc<T>) -> Self {
        Self { turbo_tasks, vc }
    }

    pub fn turbo_tasks(&self) -> &BundlerTurboTasks {
        &self.turbo_tasks
    }
}

impl<T> Deref for VcArc<T> {
    type Target = OperationVc<T>;

    fn deref(&self) -> &Self::Target {
        &self.vc
    }
}

#[napi]
pub fn root_task_dispose(
    #[napi(ts_arg_type = "{ __napiType: \"RootTask\" }")] mut root_task: External<RootTask>,
) -> napi::Result<()> {
    if let Some(task) = root_task.task_id.take() {
        root_task.turbo_tasks.dispose_root_task(task);
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

#[napi(object)]
pub struct NapiDiagnostic {
    pub category: String,
    pub name: String,
    #[napi(ts_type = "Record<string, string>")]
    pub payload: FxHashMap<String, String>,
}

impl NapiDiagnostic {
    pub fn from(diagnostic: &PlainDiagnostic) -> Self {
        Self {
            category: diagnostic.category.to_string(),
            name: diagnostic.name.to_string(),
            payload: diagnostic
                .payload
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

pub struct TurbopackResult<T: ToNapiValue> {
    pub result: T,
    pub issues: Vec<NapiIssue>,
    pub diagnostics: Vec<NapiDiagnostic>,
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
        obj.set_named_property("diagnostics", val.diagnostics)?;

        Ok(unsafe { obj.raw() })
    }
}

pub fn subscribe<T: 'static + Send + Sync, F: Future<Output = Result<T>> + Send, V: ToNapiValue>(
    turbo_tasks: BundlerTurboTasks,
    func: JsFunction,
    handler: impl 'static + Sync + Send + Clone + Fn() -> F,
    mapper: impl 'static + Sync + Send + FnMut(ThreadSafeCallContext<T>) -> napi::Result<Vec<V>>,
) -> napi::Result<External<RootTask>> {
    let func: ThreadsafeFunction<T> = func.create_threadsafe_function(0, mapper)?;
    let task_id = turbo_tasks.spawn_root_task(move || {
        let handler = handler.clone();
        let func = func.clone();
        Box::pin(async move {
            let result = handler().await;

            let status = func.call(
                result.map_err(|e| {
                    let error = PrettyPrintError(&e).to_string();
                    log_internal_error_and_inform(&error);
                    napi::Error::from_reason(error)
                }),
                ThreadsafeFunctionCallMode::NonBlocking,
            );
            if !matches!(status, Status::Ok) {
                let error = anyhow!("Error calling JS function: {}", status);
                eprintln!("{error}");
                return Err::<Vc<()>, _>(error);
            }
            Ok(Default::default())
        })
    });
    Ok(External::new(RootTask {
        turbo_tasks,
        task_id: Some(task_id),
    }))
}
