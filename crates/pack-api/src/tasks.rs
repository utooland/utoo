use std::{future::Future, sync::Arc, time::Duration};

use anyhow::Result;
use either::Either;
use napi::{JsFunction, threadsafe_function::ThreadsafeFunction};
use napi_derive::napi;
use turbo_tasks::{
    PrettyPrintError, TaskId, TurboTasks, TurboTasksApi, UpdateInfo, Vc,
    backend::TurboTasksExecutionError, task_statistics::TaskStatisticsApi, trace::TraceRawVcs,
};

use turbo_tasks_backend::{NoopBackingStorage, TurboTasksBackend};

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
use turbo_tasks_backend::DefaultBackingStorage;

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub type UtooTurboTasks =
    Arc<TurboTasks<TurboTasksBackend<Either<DefaultBackingStorage, NoopBackingStorage>>>>;

// In WASM builds there is no disk persistence; always use the noop backing storage.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub type UtooTurboTasks = Arc<TurboTasks<TurboTasksBackend<NoopBackingStorage>>>;

/// A value often wrapped in [`napi::bindgen_prelude::External`] that retains the [TurboTasks]
/// instance used by Next.js, and [various napi helpers that are passed to us from
/// JavaScript][NapiNextTurbopackCallbacks].
///
/// This is not a [`turbo_tasks::value`], and should only be used within the top-level napi layer.
/// It should not be passed to a [`turbo_tasks::function`]. For serializable information about the
/// project, use the [`next_api::project::Project`] type instead.
///
/// This type is a wrapper around an [`Arc`] and is therefore cheaply cloneable. It is [`Send`] and
/// [`Sync`].
#[derive(Clone)]
pub struct TurbopackContext {
    inner: Arc<TurboContextInner>,
}

/// A collection of helper JavaScript functions passed into
/// [`crate::pack_api::project::project_new`] and stored in the [`TurbopackContext`].
///
/// This type is [`Send`] and [`Sync`]. Callbacks are wrapped in [`ThreadsafeFunction`].
pub struct NapiTurbopackCallbacks {
    // It's a little nasty to use a `ThreadsafeFunction` for this, but we don't expect exceptions
    // to be a hot codepath.
    //
    // More ideally, we'd convert the error type in the JS thread after the execution of the future
    // when resolving the JS `Promise` object. However, doing that would add a lot more boilerplate
    // to all of our async entrypoints, and would be complicated by `FunctionRef` being `!Send` (I
    // think it could be `Send`, as long as `napi::Env` is checked at call-time, which it should be
    // anyways).
    throw_turbopack_internal_error: ThreadsafeFunction<TurbopackInternalErrorOpts>,
}

/// Arguments for `NapiTurbopackCallbacks::throw_turbopack_internal_error`.
#[napi(object)]
pub struct TurbopackInternalErrorOpts {
    pub message: String,
    pub anonymized_location: Option<String>,
}

struct TurboContextInner {
    turbo_tasks: UtooTurboTasks,
    napi_callbacks: NapiTurbopackCallbacks,
}

impl TurbopackContext {
    pub fn new(turbo_tasks: UtooTurboTasks, napi_callbacks: NapiTurbopackCallbacks) -> Self {
        TurbopackContext {
            inner: Arc::new(TurboContextInner {
                turbo_tasks,
                napi_callbacks,
            }),
        }
    }

    pub fn turbo_tasks(&self) -> &UtooTurboTasks {
        &self.inner.turbo_tasks
    }

    pub fn throw_turbopack_internal_error(
        &self,
        err: &anyhow::Error,
    ) -> impl Future<Output = napi::Error> + use<> {
        let this = self.clone();
        let message = PrettyPrintError(err).to_string();
        let downcast_root_cause_err = err.root_cause().downcast_ref::<TurboTasksExecutionError>();
        let panic_location =
            if let Some(TurboTasksExecutionError::Panic(p)) = downcast_root_cause_err {
                p.location.clone()
            } else {
                None
            };

        async move {
            this.inner
                .napi_callbacks
                .throw_turbopack_internal_error
                .call_async::<()>(Ok(TurbopackInternalErrorOpts {
                    message,
                    anonymized_location: panic_location,
                }))
                .await
                .expect_err("throwTurbopackInternalError must throw an error")
        }
    }

    pub fn throw_turbopack_internal_result<T>(
        &self,
        err: &anyhow::Error,
    ) -> impl Future<Output = napi::Result<T>> + use<T> {
        let err_fut = self.throw_turbopack_internal_error(err);
        async move { Err(err_fut.await) }
    }
}

#[napi(object)]
pub struct NapiTurbopackCallbacksJsObject {
    /// Called when we've encountered a bug in Turbopack and not in the user's code. Constructs and
    /// throws a `TurbopackInternalError` type. Logs to anonymized telemetry.
    ///
    /// As a result of the use of `ErrorStrategy::CalleeHandled`, the first argument is an error if
    /// there's a runtime conversion error. This should never happen, but if it does, the function
    /// can throw it instead.
    #[napi(ts_type = "(conversionError: Error | null, opts: TurbopackInternalErrorOpts) => never")]
    pub throw_turbopack_internal_error: JsFunction,
}

impl NapiTurbopackCallbacks {
    pub fn from_js(obj: NapiTurbopackCallbacksJsObject) -> napi::Result<Self> {
        Ok(NapiTurbopackCallbacks {
            throw_turbopack_internal_error: obj
                .throw_turbopack_internal_error
                .create_threadsafe_function(0, |ctx| {
                    // Avoid unpacking the struct into positional arguments, we really want to make
                    // sure we don't incorrectly order arguments and accidentally log a potentially
                    // PII-containing message in anonymized telemetry.
                    Ok(vec![ctx.value])
                })?,
        })
    }
}

#[derive(Clone)]
pub enum BundlerTurboTasks {
    Memory(Arc<TurboTasks<TurboTasksBackend<NoopBackingStorage>>>),
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    PersistentCaching(Arc<TurboTasks<TurboTasksBackend<DefaultBackingStorage>>>),
}

impl BundlerTurboTasks {
    pub fn dispose_root_task(&self, task: TaskId) {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.dispose_root_task(task),
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks.dispose_root_task(task)
            }
        }
    }

    pub fn spawn_root_task<T, F, Fut>(&self, functor: F) -> TaskId
    where
        T: Send,
        F: Fn() -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output = Result<Vc<T>>> + Send,
    {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.spawn_root_task(functor),
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks.spawn_root_task(functor)
            }
        }
    }

    pub async fn run_once<T: TraceRawVcs + Send + 'static>(
        &self,
        future: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T> {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.run_once(future).await,
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => turbo_tasks.run_once(future).await,
        }
    }

    pub async fn aggregated_update_info(
        &self,
        aggregation: Duration,
        timeout: Duration,
    ) -> Option<UpdateInfo> {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => {
                turbo_tasks
                    .aggregated_update_info(aggregation, timeout)
                    .await
            }
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks
                    .aggregated_update_info(aggregation, timeout)
                    .await
            }
        }
    }

    pub async fn get_or_wait_aggregated_update_info(&self, aggregation: Duration) -> UpdateInfo {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => {
                turbo_tasks
                    .get_or_wait_aggregated_update_info(aggregation)
                    .await
            }
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => {
                turbo_tasks
                    .get_or_wait_aggregated_update_info(aggregation)
                    .await
            }
        }
    }

    pub async fn stop_and_wait(&self) {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.stop_and_wait().await,
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => turbo_tasks.stop_and_wait().await,
        }
    }

    pub fn task_statistics(&self) -> &TaskStatisticsApi {
        match self {
            BundlerTurboTasks::Memory(turbo_tasks) => turbo_tasks.task_statistics(),
            #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
            BundlerTurboTasks::PersistentCaching(turbo_tasks) => turbo_tasks.task_statistics(),
        }
    }
}

/// The root of our turbopack computation.
pub struct RootTask {
    pub turbopack_ctx: TurbopackContext,
    pub task_id: Option<TaskId>,
}

impl Drop for RootTask {
    fn drop(&mut self) {
        // TODO stop the root task
    }
}
