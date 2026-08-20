//! Utilities for constructing and using the [`TurbopackContext`] type.

use std::{future::Future, sync::Arc};

use napi::{
    Env, Status, Unknown, bindgen_prelude::FunctionRef, threadsafe_function::ThreadsafeFunction,
};
use napi_derive::napi;
use pack_api::turbo_tasks::UtooTurboTasks;
use turbo_tasks::{PrettyPrintError, TaskId, backend::TurboTasksExecutionError};

/// A value often wrapped in [`napi::bindgen_prelude::External`] that retains the [TurboTasks]
/// instance used by Next.js, and [various napi helpers that are passed to us from
/// JavaScript][NapiTurbopackCallbacks].
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
    //
    // `Weak = true` so this ThreadsafeFunction doesn't keep the Node.js event loop alive after
    // shutdown.
    throw_turbopack_internal_error: ThreadsafeFunction<
        TurbopackInternalErrorOpts,
        (),
        TurbopackInternalErrorOpts,
        Status,
        /* CalleeHandled */ true,
        /* Weak */ true,
    >,
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
                .call_async(Ok(TurbopackInternalErrorOpts {
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

/// A version of [`NapiTurbopackCallbacks`] that can accepted as an argument to a napi function.
///
/// This can be converted into a [`NapiTurbopackCallbacks`] with
/// [`NapiTurbopackCallbacks::from_js`].
#[napi(object, object_to_js = false)]
pub struct NapiTurbopackCallbacksJsObject {
    /// Called when we've encountered a bug in Turbopack and not in the user's code. Constructs and
    /// throws a `TurbopackInternalError` type. Logs to anonymized telemetry.
    ///
    /// As a result of the use of `ErrorStrategy::CalleeHandled`, the first argument is an error if
    /// there's a runtime conversion error. This should never happen, but if it does, the function
    /// can throw it instead.
    #[napi(ts_type = "(conversionError: Error | null, opts: TurbopackInternalErrorOpts) => never")]
    pub throw_turbopack_internal_error: FunctionRef<Unknown<'static>, ()>,
}

impl NapiTurbopackCallbacks {
    pub fn from_js(env: &Env, obj: NapiTurbopackCallbacksJsObject) -> napi::Result<Self> {
        let throw_turbopack_internal_error = obj
            .throw_turbopack_internal_error
            .borrow_back(env)?
            .build_threadsafe_function::<TurbopackInternalErrorOpts>()
            .callee_handled::<true>()
            .weak::<true>()
            .build_callback(|ctx| Ok(ctx.value))?;
        Ok(NapiTurbopackCallbacks {
            throw_turbopack_internal_error,
        })
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
