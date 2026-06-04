use std::{
    borrow::Cow,
    io::Write,
    path::PathBuf,
    sync::LazyLock,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::pack_api::{
    endpoint::NapiWrittenEndpoint,
    turbopack_ctx::{
        NapiTurbopackCallbacks, NapiTurbopackCallbacksJsObject, RootTask, TurbopackContext,
    },
};
use anyhow::{Context, Result, anyhow, bail};
use bincode::{Decode, Encode};
use futures_util::TryFutureExt;
use napi::{
    Env, JsFunction, JsObject, Status,
    bindgen_prelude::{External, within_runtime_if_available},
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use pack_api::{
    endpoint::{Endpoint, EndpointOutputPaths, OptionEndpoint},
    entrypoint::{
        EntrypointsWithIssues, get_all_written_entrypoints_with_issues_operation,
        get_entrypoints_with_issues_operation,
    },
    hmr::{
        HmrIdentifiersWithIssues, HmrUpdateWithIssues, get_hmr_identifiers_with_issues_operation,
        hmr_update_with_issues_operation,
    },
    operation::EntrypointsOperation,
    project::{PartialProjectOptions, ProjectContainer, ProjectOptions, WatchOptions},
    source_map::get_source_map_rope,
};
use pack_core::tracing_presets::{
    TRACING_OVERVIEW_TARGETS, TRACING_TARGETS, TRACING_TURBO_TASKS_TARGETS,
    TRACING_TURBOPACK_TARGETS,
};
use tracing::Instrument;
use tracing_subscriber::{
    EnvFilter, Registry, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt,
};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{
    NonLocalValue, OperationValue, OperationVc, PrettyPrintError, ReadRef, ResolvedVc, TaskInput,
    TransientInstance, TurboTasksApi, UpdateInfo, Vc, trace::TraceRawVcs,
};
use turbo_tasks_fs::{FileContent, FileSystem, util::uri_from_file};
use turbo_unix_path::get_relative_path_to;
use turbopack_core::{
    PROJECT_FILESYSTEM_NAME, SOURCE_URL_PROTOCOL,
    source_map::{SourceMap, Token},
    version::{PartialUpdate, TotalUpdate, Update},
};
use turbopack_ecmascript_hmr_protocol::{ClientUpdateInstruction, ResourceIdentifier};
use turbopack_trace_utils::{
    exit::{ExitHandler, ExitReceiver},
    filter_layer::FilterLayer,
    raw_trace::RawTraceLayer,
    trace_writer::TraceWriter,
};

use super::{
    endpoint::ExternalEndpoint,
    utils::{NapiDiagnostic, NapiIssue, TurbopackResult, create_turbo_tasks, subscribe},
};
use crate::util::{DetachedVc, DhatProfilerGuard};

static SOURCE_MAP_PREFIX: LazyLock<String> = LazyLock::new(|| format!("{SOURCE_URL_PROTOCOL}///"));
static SOURCE_MAP_PREFIX_PROJECT: LazyLock<String> =
    LazyLock::new(|| format!("{SOURCE_URL_PROTOCOL}///[{PROJECT_FILESYSTEM_NAME}]/"));

static TRACING_INIT: std::sync::Once = std::sync::Once::new();

#[napi(object)]
#[derive(Clone, Debug)]
pub struct NapiEnvVar {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct NapiWatchOptions {
    /// Whether to watch the filesystem for file changes.
    pub enable: bool,

    /// Enable polling at a certain interval if the native file watching doesn't work (e.g.
    /// docker).
    pub poll_interval_ms: Option<f64>,

    /// Paths to ignore when watching for file changes.
    /// By default, ignores: node_modules
    pub ignored: Option<Vec<String>>,
}

#[napi(object)]
pub struct NapiProjectOptions {
    /// A root path from which all files must be nested under. Trying to access
    /// a file outside this root will fail. Think of this as a chroot.
    pub root_path: String,

    /// A path inside the root_path which contains the app/pages directories.
    pub project_path: String,

    /// Filesystem watcher options.
    pub watch: NapiWatchOptions,

    /// The contents of config.js, serialized to JSON.
    pub config: String,

    /// A map of environment variables to use when compiling code.
    pub process_env: Vec<NapiEnvVar>,

    /// The mode in which Next.js is running.
    pub dev: bool,

    /// The build id.
    pub build_id: String,

    /// Whether to enable default tracing logs.
    pub tracing: bool,

    pub pack_path: String,
}

/// [NapiProjectOptions] with all fields optional.
#[napi(object)]
pub struct NapiPartialProjectOptions {
    /// A root path from which all files must be nested under. Trying to access
    /// a file outside this root will fail. Think of this as a chroot.
    pub root_path: Option<String>,

    /// A path inside the root_path which contains the app/pages directories.
    pub project_path: Option<String>,

    /// Filesystem watcher options.
    pub watch: Option<NapiWatchOptions>,

    /// The contents of config.js, serialized to JSON.
    pub config: Option<String>,

    /// A map of environment variables to use when compiling code.
    pub process_env: Option<Vec<NapiEnvVar>>,

    /// The mode in which Next.js is running.
    pub dev: Option<bool>,

    /// The build id.
    pub build_id: Option<String>,

    /// When the code is minified, this opts out of the default mangling of
    /// local names for variables, functions etc., which can be useful for
    /// debugging/profiling purposes.
    pub no_mangling: Option<bool>,

    pub pack_path: Option<String>,
}

#[napi(object)]
pub struct NapiTurboEngineOptions {
    /// Use the new backend with persistent caching enabled.
    pub persistent_caching: Option<bool>,
    /// An upper bound of memory that turbopack will attempt to stay under.
    pub memory_limit: Option<f64>,
    /// Track dependencies between tasks. If false, any change during build will error.
    pub dependency_tracking: Option<bool>,
    /// Hint that this turbo-tasks instance is for a short-lived one-shot session.
    pub is_short_session: Option<bool>,
}

impl From<NapiWatchOptions> for WatchOptions {
    fn from(val: NapiWatchOptions) -> Self {
        WatchOptions {
            enable: val.enable,
            poll_interval: val
                .poll_interval_ms
                .filter(|interval| !interval.is_nan() && interval.is_finite() && *interval > 0.0)
                .map(|interval| Duration::from_secs_f64(interval / 1000.0)),
            ignored: val
                .ignored
                .map(|v| v.into_iter().map(|s| s.into()).collect())
                .unwrap_or_else(pack_api::project::default_ignored_paths),
        }
    }
}

impl From<NapiProjectOptions> for ProjectOptions {
    fn from(val: NapiProjectOptions) -> Self {
        ProjectOptions {
            root_path: val.root_path.into(),
            project_path: val.project_path.into(),
            watch: val.watch.into(),
            config: val.config.into(),
            process_env: val
                .process_env
                .into_iter()
                .map(|var| (var.name.into(), var.value.into()))
                .collect(),
            dev: val.dev,
            build_id: val.build_id.into(),
            pack_path: val.pack_path.into(),
        }
    }
}

impl From<NapiPartialProjectOptions> for PartialProjectOptions {
    fn from(val: NapiPartialProjectOptions) -> Self {
        PartialProjectOptions {
            root_path: val.root_path.map(From::from),
            project_path: val.project_path.map(From::from),
            watch: val.watch.map(From::from),
            config: val.config.map(From::from),
            process_env: val.process_env.map(|env| {
                env.into_iter()
                    .map(|var| (var.name.into(), var.value.into()))
                    .collect()
            }),
            build_id: val.build_id.map(From::from),
            pack_path: val.pack_path.map(From::from),
        }
    }
}

pub struct ProjectInstance {
    pub turbopack_ctx: TurbopackContext,
    pub container: ResolvedVc<ProjectContainer>,
    pub exit_receiver: tokio::sync::Mutex<Option<ExitReceiver>>,
}

#[napi(ts_return_type = "Promise<{ __napiType: \"Project\" }>")]
pub fn project_new(
    env: Env,
    options: NapiProjectOptions,
    turbo_engine_options: NapiTurboEngineOptions,
    napi_callbacks: NapiTurbopackCallbacksJsObject,
) -> napi::Result<JsObject> {
    let napi_callbacks = NapiTurbopackCallbacks::from_js(&env, napi_callbacks)?;
    let (exit, exit_receiver) = ExitHandler::new_receiver();

    if let Some(dhat_profiler) = DhatProfilerGuard::try_init() {
        exit.on_exit(async move {
            tokio::task::spawn_blocking(move || drop(dhat_profiler))
                .await
                .unwrap()
        });
    }

    let mut trace = std::env::var("TURBOPACK_TRACING")
        .ok()
        .filter(|v| !v.is_empty());

    let tracing_chrome = std::env::var("TRACING_CHROME")
        .ok()
        .filter(|v| !v.is_empty());

    if cfg!(feature = "tokio-console") && trace.is_none() && tracing_chrome.is_none() {
        // ensure `trace` is set to *something* so that the `tokio-console` feature works, otherwise
        // you just get empty output from `tokio-console`, which can be confusing.
        trace = Some("overview".to_owned());
    }

    if let Some(mut trace) = trace {
        // Trace presets
        match trace.as_str() {
            "overview" | "1" => {
                trace = TRACING_OVERVIEW_TARGETS.join(",");
            }
            "pack" => {
                trace = TRACING_TARGETS.join(",");
            }
            "turbopack" => {
                trace = TRACING_TURBOPACK_TARGETS.join(",");
            }
            "turbo-tasks" => {
                trace = TRACING_TURBO_TASKS_TARGETS.join(",");
            }
            _ => {}
        }

        let subscriber = Registry::default();

        if cfg!(feature = "tokio-console") {
            trace = format!("{trace},tokio=trace,runtime=trace");
        }
        #[cfg(feature = "tokio-console")]
        let subscriber = subscriber.with(console_subscriber::spawn());

        let subscriber = subscriber.with(FilterLayer::try_new(&trace).unwrap());

        let internal_dir = PathBuf::from(&options.project_path).join(".turbopack");
        std::fs::create_dir_all(&internal_dir)
            .context("Unable to create .turbopack directory")
            .unwrap();
        let trace_file = internal_dir.join(".trace-turbopack");
        let trace_writer = std::fs::File::create(trace_file.clone()).unwrap();
        let (trace_writer, trace_writer_guard) = TraceWriter::new(trace_writer);
        let subscriber = subscriber.with(RawTraceLayer::new(trace_writer));

        let mut tracing_server_handle: Option<JoinHandle<()>> = None;
        let trace_server = std::env::var("TURBOPACK_TRACE_SERVER").ok();
        if trace_server.is_some() {
            tracing_server_handle = Some(thread::spawn(move || {
                turbopack_trace_server::start_turbopack_trace_server(trace_file, None);
            }));
            println!(
                "Turbopack trace server started. View trace at https://turbo-trace-viewer.vercel.app/"
            );
        }

        exit.on_exit(async move {
            tokio::task::spawn_blocking(move || {
                drop(trace_writer_guard);
                if let Some(tracing_server_handle) = tracing_server_handle {
                    tracing_server_handle.join().unwrap();
                }
            })
            .await
            .unwrap();
        });

        TRACING_INIT.call_once(|| {
            subscriber.init();
        });
    } else if let Some(chrome_file) = tracing_chrome {
        let mut builder = tracing_chrome::ChromeLayerBuilder::new().include_args(false);
        if chrome_file != "1" && chrome_file != "true" {
            builder = builder.file(chrome_file);
        }
        let (chrome_layer, guard) = builder.build();
        exit.on_exit(async move {
            tokio::task::spawn_blocking(move || drop(guard))
                .await
                .unwrap();
        });

        TRACING_INIT.call_once(|| {
            tracing_subscriber::registry()
                .with(EnvFilter::new("info"))
                .with(chrome_layer)
                .init();
        });
    } else if options.tracing {
        TRACING_INIT.call_once(|| {
            let env_filter = EnvFilter::try_from_default_env();
            let env_filter_enabled = env_filter.is_ok();
            tracing_subscriber::fmt()
                .with_env_filter(env_filter.unwrap_or_else(|_| {
                    EnvFilter::new("pack_napi=info,pack_api=info,pack_core=info")
                }))
                .with_target(env_filter_enabled)
                .with_span_events(if env_filter_enabled {
                    FmtSpan::CLOSE
                } else {
                    FmtSpan::NONE
                })
                .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                    "%Y-%m-%d %H:%M:%S.%3f".to_string(),
                ))
                .init();
        });
    }

    env.spawn_future(
        async move {
            let memory_limit = turbo_engine_options
                .memory_limit
                .map(|m| m as usize)
                .unwrap_or(usize::MAX);
            let persistent_caching = turbo_engine_options.persistent_caching.unwrap_or_default();
            let dependency_tracking = turbo_engine_options.dependency_tracking.unwrap_or(true);
            let is_short_session = turbo_engine_options.is_short_session.unwrap_or(false);
            let turbo_tasks = create_turbo_tasks(
                PathBuf::from(&options.project_path),
                persistent_caching,
                memory_limit,
                dependency_tracking,
                is_short_session,
            )?;
            let turbopack_ctx = TurbopackContext::new(turbo_tasks.clone(), napi_callbacks);

            if let Some(stats_path) = std::env::var_os("TURBOPACK_TASK_STATISTICS") {
                let task_stats = turbo_tasks.task_statistics().enable().clone();
                exit.on_exit(async move {
                    tokio::task::spawn_blocking(move || {
                        let mut file = std::fs::File::create(&stats_path)
                            .with_context(|| format!("failed to create or open {stats_path:?}"))?;
                        serde_json::to_writer(&file, &task_stats)
                            .context("failed to serialize or write task statistics")?;
                        file.flush().context("failed to flush file")
                    })
                    .await
                    .unwrap()
                    .unwrap();
                });
            }
            let options = ProjectOptions::from(options);
            let container = turbo_tasks
                .run(async move {
                    let container_op =
                        ProjectContainer::new_operation(rcstr!("utoopack"), options.dev);
                    ProjectContainer::initialize(container_op, options).await?;
                    container_op.resolve().strongly_consistent().await
                })
                .or_else(|e| turbopack_ctx.throw_turbopack_internal_result(&e.into()))
                .await?;

            Ok(External::new_with_size_hint(
                ProjectInstance {
                    turbopack_ctx,
                    container,
                    exit_receiver: tokio::sync::Mutex::new(Some(exit_receiver)),
                },
                100,
            ))
        }
        .instrument(tracing::info_span!("create project")),
    )
}

#[napi]
pub async fn project_update(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    options: NapiPartialProjectOptions,
) -> napi::Result<()> {
    let ctx = &project.turbopack_ctx;
    let options = options.into();
    let container = project.container;

    ctx.turbo_tasks()
        .run(async move { container.update(options).await })
        .or_else(|e| ctx.throw_turbopack_internal_result(&e.into()))
        .await
}

/// Runs exit handlers for the project registered using the [`ExitHandler`] API.
///
/// This is called by `project_shutdown`, so if you're calling that API, you shouldn't call this
/// one.
#[napi]
pub async fn project_on_exit(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
) {
    project_on_exit_internal(&project).await
}

async fn project_on_exit_internal(project: &ProjectInstance) {
    let exit_receiver = project.exit_receiver.lock().await.take();
    exit_receiver
        .expect("`project.onExitSync` must only be called once")
        .run_exit_handler()
        .await;
}

/// Runs `project_on_exit`, and then waits for turbo_tasks to gracefully shut down.
///
/// This is used in builds where it's important that we completely persist turbo-tasks to disk, but
/// it's skipped in the development server (`project_on_exit` is used instead with a short timeout),
/// where we prioritize fast exit and user responsiveness over all else.
#[napi]
pub async fn project_shutdown(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
) {
    project.turbopack_ctx.turbo_tasks().stop_and_wait().await;
    project_on_exit_internal(&project).await;
}

#[napi(object)]
pub struct NapiEntrypoints {
    pub apps: Option<Vec<External<ExternalEndpoint>>>,
    pub libraries: Option<Vec<External<ExternalEndpoint>>>,
    pub app_paths: Option<Vec<NapiWrittenEndpoint>>,
    pub library_paths: Option<Vec<NapiWrittenEndpoint>>,
}

impl NapiEntrypoints {
    fn from_entrypoints_op(
        entrypoints: &EntrypointsOperation,
        turbopack_ctx: &TurbopackContext,
    ) -> Result<Self> {
        let make_endpoint =
            |op| External::new(ExternalEndpoint(DetachedVc::new(turbopack_ctx.clone(), op)));
        Ok(NapiEntrypoints {
            apps: Some(
                entrypoints
                    .apps
                    .iter()
                    .copied()
                    .map(make_endpoint)
                    .collect(),
            ),
            libraries: Some(
                entrypoints
                    .libraries
                    .iter()
                    .copied()
                    .map(make_endpoint)
                    .collect(),
            ),
            app_paths: None,
            library_paths: None,
        })
    }
}

async fn collect_endpoint_output_paths(
    endpoints: &[OperationVc<OptionEndpoint>],
) -> Result<Vec<EndpointOutputPaths>> {
    let mut paths = Vec::with_capacity(endpoints.len());

    for endpoint in endpoints {
        let endpoint = endpoint.connect().await?;
        let output_paths = if let Some(endpoint) = *endpoint {
            let output = endpoint.output().await?;
            let output_paths = output.output_paths.await?;
            ReadRef::into_owned(output_paths)
        } else {
            EndpointOutputPaths::NotFound
        };
        paths.push(output_paths);
    }

    Ok(paths)
}

#[tracing::instrument(level = "info", name = "write all entrypoints to disk", skip_all)]
#[napi]
pub async fn project_write_all_entrypoints_to_disk(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
) -> napi::Result<TurbopackResult<NapiEntrypoints>> {
    let start = Instant::now();
    let ctx = &project.turbopack_ctx;
    let container = project.container;
    let tt = ctx.turbo_tasks();

    let (entrypoints, app_paths, library_paths, issues, diags) = tt
        .run(async move {
            let entrypoints_with_issues_op =
                get_all_written_entrypoints_with_issues_operation(container);
            // Read and compile the files
            let EntrypointsWithIssues {
                entrypoints,
                issues,
                diagnostics,
                effects,
            } = &*entrypoints_with_issues_op
                .read_strongly_consistent()
                .await?;
            // Apply phase side effects. Asset emission is performed once at the end.
            effects.apply().await?;
            let app_paths = collect_endpoint_output_paths(&entrypoints.apps).await?;
            let library_paths = collect_endpoint_output_paths(&entrypoints.libraries).await?;

            Ok((
                entrypoints.clone(),
                app_paths,
                library_paths,
                issues.iter().cloned().collect::<Vec<_>>(),
                diagnostics.iter().cloned().collect::<Vec<_>>(),
            ))
        })
        .or_else(|e| ctx.throw_turbopack_internal_result(&e.into()))
        .await?;

    tracing::info!("All project entrypoints wrote to disk.");

    let mut napi_entrypoints =
        NapiEntrypoints::from_entrypoints_op(&entrypoints, &project.turbopack_ctx)?;
    napi_entrypoints.app_paths = Some(
        app_paths
            .into_iter()
            .map(|path| NapiWrittenEndpoint::from(Some(path)))
            .collect(),
    );
    napi_entrypoints.library_paths = Some(
        library_paths
            .into_iter()
            .map(|path| NapiWrittenEndpoint::from(Some(path)))
            .collect(),
    );

    tracing::info!("Compile done in {:?}", start.elapsed());

    Ok(TurbopackResult {
        result: napi_entrypoints,
        issues: issues.iter().map(|i| NapiIssue::from(&**i)).collect(),
        diagnostics: diags.iter().map(|d| NapiDiagnostic::from(d)).collect(),
    })
}

#[napi(ts_return_type = "{ __napiType: \"RootTask\" }")]
pub fn project_entrypoints_subscribe(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    func: JsFunction,
) -> napi::Result<External<RootTask>> {
    let turbopack_ctx = project.turbopack_ctx.clone();
    let container = project.container;
    subscribe(
        turbopack_ctx.clone(),
        func,
        move || {
            async move {
                let entrypoints_with_issues_op = get_entrypoints_with_issues_operation(container);
                let EntrypointsWithIssues {
                    entrypoints,
                    issues,
                    diagnostics,
                    effects,
                } = &*entrypoints_with_issues_op
                    .read_strongly_consistent()
                    .await?;
                effects.apply().await?;
                Ok((entrypoints.clone(), issues.clone(), diagnostics.clone()))
            }
            .instrument(tracing::trace_span!("entrypoints subscription"))
        },
        move |ctx| {
            let (entrypoints, issues, diags) = ctx.value;

            Ok(vec![TurbopackResult {
                result: NapiEntrypoints::from_entrypoints_op(&entrypoints, &turbopack_ctx)?,
                issues: issues
                    .iter()
                    .map(|issue| NapiIssue::from(&**issue))
                    .collect(),
                diagnostics: diags.iter().map(|d| NapiDiagnostic::from(d)).collect(),
            }])
        },
    )
}

#[tracing::instrument(level = "debug", name = "get HMR events", skip(project, func))]
#[napi(ts_return_type = "{ __napiType: \"RootTask\" }")]
pub fn project_hmr_events(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    identifier: RcStr,
    func: JsFunction,
) -> napi::Result<External<RootTask>> {
    let turbopack_ctx = project.turbopack_ctx.clone();
    let project = project.container;
    let session = TransientInstance::new(());
    subscribe(
        turbopack_ctx,
        func,
        {
            let outer_identifier = identifier.clone();
            let session = session.clone();
            move || {
                let identifier: RcStr = outer_identifier.clone();
                let session = session.clone();
                async move {
                    let project = project.project().to_resolved().await?;
                    let state = project
                        .hmr_version_state(identifier.clone(), session)
                        .to_resolved()
                        .await?;

                    let update_op =
                        hmr_update_with_issues_operation(project, identifier.clone(), state);
                    let update = update_op.read_strongly_consistent().await?;
                    let HmrUpdateWithIssues {
                        update,
                        issues,
                        diagnostics,
                        effects,
                    } = &*update;
                    effects.apply().await?;
                    match &**update {
                        Update::Missing | Update::None => {}
                        Update::Total(TotalUpdate { to }) => {
                            state.set(to.clone()).await?;
                        }
                        Update::Partial(PartialUpdate { to, .. }) => {
                            state.set(to.clone()).await?;
                        }
                    }
                    Ok((Some(update.clone()), issues.clone(), diagnostics.clone()))
                }
            }
        },
        move |ctx| {
            let (update, issues, diags) = ctx.value;

            let napi_issues = issues
                .iter()
                .map(|issue| NapiIssue::from(&**issue))
                .collect();
            let update_issues = issues
                .iter()
                .map(|issue| (&**issue).into())
                .collect::<Vec<_>>();

            let identifier = ResourceIdentifier {
                path: identifier.clone(),
                headers: None,
            };
            let update = match update.as_deref() {
                None | Some(Update::Missing) | Some(Update::Total(_)) => {
                    ClientUpdateInstruction::restart(&identifier, &update_issues)
                }
                Some(Update::Partial(update)) => ClientUpdateInstruction::partial(
                    &identifier,
                    &update.instruction,
                    &update_issues,
                ),
                Some(Update::None) => ClientUpdateInstruction::issues(&identifier, &update_issues),
            };

            Ok(vec![TurbopackResult {
                result: ctx.env.to_js_value(&update)?,
                issues: napi_issues,
                diagnostics: diags.iter().map(|d| NapiDiagnostic::from(d)).collect(),
            }])
        },
    )
}

#[napi(object)]
struct HmrIdentifiers {
    pub identifiers: Vec<String>,
}

#[napi(ts_return_type = "{ __napiType: \"RootTask\" }")]
pub fn project_hmr_identifiers_subscribe(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    func: JsFunction,
) -> napi::Result<External<RootTask>> {
    let turbopack_ctx = project.turbopack_ctx.clone();
    let container = project.container;
    subscribe(
        turbopack_ctx,
        func,
        move || async move {
            let hmr_identifiers_with_issues_op =
                get_hmr_identifiers_with_issues_operation(container);
            let HmrIdentifiersWithIssues {
                identifiers,
                issues,
                diagnostics,
                effects,
            } = &*hmr_identifiers_with_issues_op
                .read_strongly_consistent()
                .await?;
            effects.apply().await?;

            Ok((identifiers.clone(), issues.clone(), diagnostics.clone()))
        },
        move |ctx| {
            let (identifiers, issues, diagnostics) = ctx.value;

            Ok(vec![TurbopackResult {
                result: HmrIdentifiers {
                    identifiers: identifiers
                        .iter()
                        .map(|ident| ident.to_string())
                        .collect::<Vec<_>>(),
                },
                issues: issues
                    .iter()
                    .map(|issue| NapiIssue::from(&**issue))
                    .collect(),
                diagnostics: diagnostics
                    .iter()
                    .map(|d| NapiDiagnostic::from(d))
                    .collect(),
            }])
        },
    )
}

pub enum UpdateMessage {
    Start,
    End(UpdateInfo),
}

#[napi(object)]
struct NapiUpdateMessage {
    pub update_type: String,
    pub value: Option<NapiUpdateInfo>,
}

impl From<UpdateMessage> for NapiUpdateMessage {
    fn from(update_message: UpdateMessage) -> Self {
        match update_message {
            UpdateMessage::Start => NapiUpdateMessage {
                update_type: "start".to_string(),
                value: None,
            },
            UpdateMessage::End(info) => NapiUpdateMessage {
                update_type: "end".to_string(),
                value: Some(info.into()),
            },
        }
    }
}

#[napi(object)]
struct NapiUpdateInfo {
    pub duration: u32,
    pub tasks: u32,
}

impl From<UpdateInfo> for NapiUpdateInfo {
    fn from(update_info: UpdateInfo) -> Self {
        Self {
            duration: update_info.duration.as_millis() as u32,
            tasks: update_info.tasks as u32,
        }
    }
}

/// Subscribes to lifecycle events of the compilation.
///
/// Emits an [UpdateMessage::Start] event when any computation starts.
/// Emits an [UpdateMessage::End] event when there was no computation for the
/// specified time (`aggregation_ms`). The [UpdateMessage::End] event contains
/// information about the computations that happened since the
/// [UpdateMessage::Start] event. It contains the duration of the computation
/// (excluding the idle time that was spend waiting for `aggregation_ms`), and
/// the number of tasks that were executed.
///
/// The signature of the `func` is `(update_message: UpdateMessage) => void`.
#[napi]
pub fn project_update_info_subscribe(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    aggregation_ms: u32,
    func: JsFunction,
) -> napi::Result<()> {
    let func: ThreadsafeFunction<UpdateMessage> = func.create_threadsafe_function(0, |ctx| {
        let message = ctx.value;
        Ok(vec![NapiUpdateMessage::from(message)])
    })?;
    let turbo_tasks = project.turbopack_ctx.turbo_tasks().clone();
    tokio::spawn(async move {
        loop {
            let update_info = turbo_tasks
                .aggregated_update_info(Duration::ZERO, Duration::ZERO)
                .await;

            func.call(
                Ok(UpdateMessage::Start),
                ThreadsafeFunctionCallMode::NonBlocking,
            );

            let update_info = match update_info {
                Some(update_info) => update_info,
                None => {
                    turbo_tasks
                        .get_or_wait_aggregated_update_info(Duration::from_millis(
                            aggregation_ms.into(),
                        ))
                        .await
                }
            };

            let status = func.call(
                Ok(UpdateMessage::End(update_info)),
                ThreadsafeFunctionCallMode::NonBlocking,
            );

            if !matches!(status, Status::Ok) {
                let error = anyhow!("Error calling JS function: {}", status);
                eprintln!("{error}");
                break;
            }
        }
    });
    Ok(())
}

#[napi(object)]
#[derive(
    Clone,
    Debug,
    Eq,
    Hash,
    NonLocalValue,
    OperationValue,
    PartialEq,
    TaskInput,
    TraceRawVcs,
    Encode,
    Decode,
)]
pub struct StackFrame {
    pub is_server: bool,
    pub is_internal: Option<bool>,
    pub original_file: Option<RcStr>,
    pub file: RcStr,
    /// 1-indexed, unlike source map tokens
    pub line: Option<u32>,
    /// 1-indexed, unlike source map tokens
    pub column: Option<u32>,
    pub method_name: Option<RcStr>,
}

#[turbo_tasks::value(transparent)]
#[derive(Clone)]
pub struct OptionStackFrame(Option<StackFrame>);

#[turbo_tasks::function(operation)]
pub async fn project_trace_source_operation(
    container: ResolvedVc<ProjectContainer>,
    frame: StackFrame,
    current_directory_file_url: RcStr,
) -> Result<Vc<OptionStackFrame>> {
    let Some(map) =
        &*SourceMap::new_from_rope_cached(get_source_map_rope(*container, frame.file)).await?
    else {
        return Ok(Vc::cell(None));
    };

    let Some(line) = frame.line else {
        return Ok(Vc::cell(None));
    };

    let token = map.lookup_token(
        line.saturating_sub(1),
        frame.column.unwrap_or(1).saturating_sub(1),
    );

    let (original_file, line, column, method_name) = match token {
        Token::Original(token) => (
            match urlencoding::decode(&token.original_file)? {
                Cow::Borrowed(_) => token.original_file,
                Cow::Owned(original_file) => RcStr::from(original_file),
            },
            // JS stack frames are 1-indexed, source map tokens are 0-indexed
            Some(token.original_line + 1),
            Some(token.original_column + 1),
            token.name,
        ),
        Token::Synthetic(token) => {
            let Some(original_file) = token.guessed_original_file else {
                return Ok(Vc::cell(None));
            };
            (original_file, None, None, None)
        }
    };

    let project_root_uri =
        uri_from_file(container.project().project_root().owned().await?, None).await? + "/";
    let (file, original_file, is_internal) =
        if let Some(source_file) = original_file.strip_prefix(&project_root_uri) {
            // Client code uses file://
            (
                RcStr::from(
                    get_relative_path_to(&current_directory_file_url, &original_file)
                        // TODO(sokra) remove this to include a ./ here to make it a relative path
                        .trim_start_matches("./"),
                ),
                Some(RcStr::from(source_file)),
                false,
            )
        } else if let Some(source_file) = original_file.strip_prefix(&*SOURCE_MAP_PREFIX_PROJECT) {
            // Server code uses turbopack:///[project]
            // TODO should this also be file://?
            (
                RcStr::from(
                    get_relative_path_to(
                        &current_directory_file_url,
                        &format!("{project_root_uri}{source_file}"),
                    )
                    // TODO(sokra) remove this to include a ./ here to make it a relative path
                    .trim_start_matches("./"),
                ),
                Some(RcStr::from(source_file)),
                false,
            )
        } else if let Some(source_file) = original_file.strip_prefix(&*SOURCE_MAP_PREFIX) {
            // All other code like turbopack:///[turbopack] is internal code
            // TODO(veil): Should the protocol be preserved?
            (RcStr::from(source_file), None, true)
        } else {
            bail!(
                "Original file ({}) outside project ({})",
                original_file,
                project_root_uri
            )
        };

    Ok(Vc::cell(Some(StackFrame {
        file,
        original_file,
        method_name,
        line,
        column,
        is_server: frame.is_server,
        is_internal: Some(is_internal),
    })))
}

#[napi]
pub async fn project_trace_source(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    frame: StackFrame,
    current_directory_file_url: String,
) -> napi::Result<Option<StackFrame>> {
    let turbo_tasks = project.turbopack_ctx.turbo_tasks().clone();
    let container = project.container;
    let traced_frame = turbo_tasks
        .run(async move {
            project_trace_source_operation(
                container,
                frame,
                RcStr::from(current_directory_file_url),
            )
            .read_strongly_consistent()
            .await
        })
        .await
        .map_err(|e| napi::Error::from_reason(PrettyPrintError(&e.into()).to_string()))?;
    Ok(ReadRef::into_owned(traced_frame))
}

#[napi]
pub async fn project_get_source_for_asset(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    file_path: String,
) -> napi::Result<Option<String>> {
    let turbo_tasks = project.turbopack_ctx.turbo_tasks().clone();
    let source = turbo_tasks
        .run(async move {
            let source_content = &*project
                .container
                .project()
                .project_path()
                .await?
                .fs()
                .root()
                .await?
                .join(&file_path)?
                .read()
                .await?;

            let FileContent::Content(source_content) = source_content else {
                bail!("Cannot find source for asset {}", file_path);
            };

            Ok(Some(source_content.content().to_str()?.into_owned()))
        })
        .await
        .map_err(|e| napi::Error::from_reason(PrettyPrintError(&e.into()).to_string()))?;

    Ok(source)
}

#[napi]
pub async fn project_get_source_map(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    file_path: RcStr,
) -> napi::Result<Option<String>> {
    let turbo_tasks = project.turbopack_ctx.turbo_tasks().clone();
    let container = project.container;

    let source_map = turbo_tasks
        .run(async move {
            let source_map = get_source_map_rope_operation(container, file_path)
                .read_strongly_consistent()
                .await?;
            let Some(map) = source_map.as_content() else {
                return Ok(None);
            };
            Ok(Some(map.content().to_str()?.to_string()))
        })
        .await
        .map_err(|e| napi::Error::from_reason(PrettyPrintError(&e.into()).to_string()))?;

    Ok(source_map)
}

#[turbo_tasks::function(operation)]
pub fn get_source_map_rope_operation(
    container: ResolvedVc<ProjectContainer>,
    file_path: RcStr,
) -> Vc<FileContent> {
    get_source_map_rope(*container, file_path)
}

#[napi]
pub fn project_get_source_map_sync(
    #[napi(ts_arg_type = "{ __napiType: \"Project\" }")] project: External<ProjectInstance>,
    file_path: RcStr,
) -> napi::Result<Option<String>> {
    within_runtime_if_available(|| {
        tokio::runtime::Handle::current().block_on(project_get_source_map(project, file_path))
    })
}
