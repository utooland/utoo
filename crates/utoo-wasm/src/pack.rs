use anyhow::{Context, Result};
use pack_api::{
    endpoint::Endpoint,
    entrypoint::{
        get_all_written_entrypoints_with_issues_operation, get_entrypoints_with_issues_operation,
        EntrypointsWithIssues,
    },
    hmr::{hmr_update_with_issues_operation, HmrUpdateWithIssues},
    project::{ProjectContainer, ProjectOptions, WatchOptions},
    turbo_tasks::UtooTurboTasks,
    utils::StyledStringSerialize,
};
use parking_lot::RwLock;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::{ops::Deref, path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tokio::{sync::mpsc::unbounded_channel, time::Instant};
use turbo_rcstr::{rcstr, RcStr};
use turbo_tasks::TaskId;
use turbo_tasks::{
    read_strongly_consistent_and_apply_effects, Completion, OperationVc, PrettyPrintError,
    ReadConsistency, ResolvedVc, TransientInstance, TurboTasks, Vc,
};
use turbo_tasks_backend::{noop_backing_storage, BackendOptions, TurboTasksBackend};
use turbo_tasks_fs::FileContent;
use turbopack_core::{
    issue::{PlainIssue, PlainIssueSource, PlainSource},
    source_pos::SourcePos as SourcePosInner,
    version::{PartialUpdate, TotalUpdate, Update, VersionState},
};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::fs::Fs;
use crate::tokio_runtime::runtime;

// https://github.com/dtolnay/inventory/blob/f1be63af0ab00ce41c39f1dd190a624de949c684/src/lib.rs#L105-L148
unsafe extern "C" {
    pub fn __wasm_call_ctors();
}

/// Global pack project instance
static GLOBAL_PACK_PROJECT: RwLock<Option<Arc<PackProject>>> = RwLock::new(None);

/// A root task handle that keeps the turbo-tasks subscription alive.
/// This must be held by JS to keep the subscription active.
#[wasm_bindgen]
pub struct RootTask {
    #[allow(dead_code)]
    turbo_tasks: UtooTurboTasks,
    #[allow(dead_code)]
    task_id: TaskId,
}

impl Drop for RootTask {
    fn drop(&mut self) {
        tracing::debug!("RootTask dropped, subscription will be stopped");
        // TODO: dispose the root task
    }
}

pub struct PackProject {
    pub turbo_tasks: UtooTurboTasks,
    pub container: ResolvedVc<ProjectContainer>,
    pub dev: bool,
}

/// Options for the build operation, exposed to JS with auto-generated typings.
#[wasm_bindgen]
pub struct BuildOptions {
    /// Optional bundler config (the content of `utoopack.json`).
    /// When provided, the config file is not read from disk.
    config: Option<JsValue>,
    /// When true, drops the existing global project and creates a fresh instance.
    #[wasm_bindgen]
    pub cleanup: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            config: None,
            cleanup: false,
        }
    }
}

#[wasm_bindgen]
impl BuildOptions {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    #[wasm_bindgen(getter)]
    pub fn config(&self) -> JsValue {
        self.config.clone().unwrap_or(JsValue::UNDEFINED)
    }

    #[wasm_bindgen(setter)]
    pub fn set_config(&mut self, val: JsValue) {
        if val.is_undefined() || val.is_null() {
            self.config = None;
        } else {
            self.config = Some(val);
        }
    }
}

impl BuildOptions {
    /// Extract the config as a JSON string for the internal API.
    pub(crate) fn config_string(&self) -> Option<String> {
        self.config.as_ref().and_then(|val| {
            js_sys::JSON::stringify(val)
                .ok()
                .and_then(|s| s.as_string().filter(|s_str| !s_str.is_empty()))
        })
    }
}

pub fn create_turbo_tasks() -> Result<UtooTurboTasks> {
    Ok(TurboTasks::new(
        turbo_tasks_backend::TurboTasksBackend::new(
            turbo_tasks_backend::BackendOptions {
                storage_mode: None,
                dependency_tracking: true,
                ..Default::default()
            },
            noop_backing_storage(),
        ),
    ))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: String,
    pub stage: String,
    pub file_path: String,
    pub title: serde_json::Value,
    pub description: Option<serde_json::Value>,
    pub detail: Option<serde_json::Value>,
    pub source: Option<IssueSource>,
    pub documentation_link: String,
    pub import_traces: serde_json::Value,
}

impl From<&PlainIssue> for Issue {
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

#[derive(Serialize, Deserialize)]
pub struct IssueSource {
    pub source: Source,
    pub range: Option<IssueSourceRange>,
}

impl From<&PlainIssueSource> for IssueSource {
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

#[derive(Serialize, Deserialize)]
pub struct Source {
    pub ident: String,
    pub content: Option<String>,
}

impl From<&PlainSource> for Source {
    fn from(source: &PlainSource) -> Self {
        Self {
            ident: source.ident.to_string(),
            content: match &*source.content {
                FileContent::Content(content) => match content.content().to_str() {
                    std::result::Result::Ok(str) => Some(str.into_owned()),
                    Err(_) => None,
                },
                FileContent::NotFound => None,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct IssueSourceRange {
    pub start: SourcePos,
    pub end: SourcePos,
}

impl From<&(SourcePosInner, SourcePosInner)> for IssueSourceRange {
    fn from((start, end): &(SourcePosInner, SourcePosInner)) -> Self {
        Self {
            start: (*start).into(),
            end: (*end).into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SourcePos {
    pub line: u32,
    pub column: u32,
}

impl From<SourcePosInner> for SourcePos {
    fn from(pos: SourcePosInner) -> Self {
        Self {
            line: pos.line,
            column: pos.column,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct TurbopackResult {
    pub issues: Vec<Issue>,
}

#[cfg(feature = "utoopack")]
#[wasm_bindgen(js_name = "registerWorkerScheduler")]
pub fn register_worker_scheduler(creator: js_sys::Function, terminator: js_sys::Function) {
    wasm_bindgen_futures::spawn_local(
        turbopack_node::worker_pool::web_worker::register_worker_scheduler(creator, terminator),
    );
}

#[cfg(feature = "utoopack")]
#[wasm_bindgen(js_name = "workerCreated")]
pub fn worker_created(worker_id: u32) {
    turbopack_node::worker_pool::web_worker::worker_created(worker_id);
}

/// Initialize or reinitialize the pack project with the given dev mode.
///
/// If `config` is `Some`, it is used directly as the bundler configuration
/// (the JSON content that would normally come from `utoopack.json`).
/// If `None`, the function reads `utoopack.json` from the project root,
/// falling back to an empty `{}` when the file does not exist.
pub async fn init_pack_project(config: Option<String>, dev: bool) -> Result<()> {
    // Only reinitialize if the mode (build vs dev) has changed
    if let Some(project) = &*GLOBAL_PACK_PROJECT.read() {
        if project.dev == dev {
            return Ok(());
        }
    }

    // Drop the old project
    GLOBAL_PACK_PROJECT.write().take();

    let cwd = crate::pm::with_project(|p| p.cwd().to_string_lossy().to_string());
    let project_root = if cwd.starts_with('/') {
        cwd
    } else {
        tokio_fs_ext::current_dir()?
            .join(cwd)
            .to_string_lossy()
            .to_string()
    };

    let mut config: Value = if let Some(cfg) = config {
        serde_json::from_str(&cfg).context("Invalid JSON configuration provided")?
    } else {
        let config_path = PathBuf::from(&project_root).join("utoopack.json");
        if tokio_fs_ext::metadata(&config_path).await.is_ok() {
            let config_str = Fs::read_to_string(&config_path.to_string_lossy())
                .await
                .ok()
                .unwrap_or_default();
            serde_json::from_str(&config_str).unwrap_or(json!({}))
        } else {
            json!({})
        }
    };

    // Inject mode and HMR settings
    config["mode"] = json!(if dev { "development" } else { "production" });
    if dev {
        if config.get("devServer").is_none() {
            config["devServer"] = json!({});
        }
        config["devServer"]["hot"] = json!(true);
    }

    let project_path: RcStr = project_root.into();
    let options = ProjectOptions {
        root_path: project_path.clone(),
        project_path: project_path.clone(),
        config: serde_json::to_string(&config)?.into(),
        build_id: project_path.clone(),
        watch: WatchOptions {
            enable: true,
            ignored: vec![],
            ..Default::default()
        },
        dev,
        pack_path: rcstr!("./"),
        process_env: Default::default(),
    };

    tracing::debug!("ProjectOptions: {:?}", options);

    let pack_context = runtime()
        .spawn(async move {
            let turbo_tasks = create_turbo_tasks()?;
            let container = turbo_tasks
                .run(async move {
                    let container_op =
                        ProjectContainer::new_operation("utoopack-web".into(), options.dev);
                    ProjectContainer::initialize(container_op, options).await?;

                    container_op.resolve().strongly_consistent().await
                })
                .await?;

            Ok::<PackProject, anyhow::Error>(PackProject {
                turbo_tasks,
                container,
                dev,
            })
        })
        .await
        .context("fail to initialize pack project")??;

    *GLOBAL_PACK_PROJECT.write() = Some(Arc::new(pack_context));
    Ok(())
}

/// Build operation.
pub async fn build(options: BuildOptions) -> std::result::Result<JsValue, wasm_bindgen::JsError> {
    use wasm_bindgen::JsError;

    if options.cleanup {
        tracing::info!("cleanup: dropping existing pack project");
        GLOBAL_PACK_PROJECT.write().take();
    }

    init_pack_project(options.config_string(), false)
        .await
        .map_err(|e| JsError::new(&PrettyPrintError(&e).to_string()))?;

    let pack_project = GLOBAL_PACK_PROJECT
        .read()
        .as_ref()
        .cloned()
        .ok_or_else(|| JsError::new("pack project not initialized"))?;

    runtime()
        .spawn(async move {
            let start = Instant::now();
            let turbo_tasks = pack_project.turbo_tasks.clone();
            let container = pack_project.container;
            let (entrypoints, issues) = turbo_tasks
                .run(async move {
                    let entrypoints_with_issues_op =
                        get_all_written_entrypoints_with_issues_operation(container);
                    let entrypoints_with_issues = read_strongly_consistent_and_apply_effects(
                        entrypoints_with_issues_op,
                        |v| &v.effects,
                    )
                    .await?;

                    let EntrypointsWithIssues {
                        entrypoints,
                        issues,
                        effects: _,
                    } = &*entrypoints_with_issues;

                    Ok((entrypoints.clone(), issues.clone()))
                })
                .await?;

            tracing::info!("build finished in {:?}", start.elapsed());

            let result = TurbopackResult {
                issues: issues.iter().map(|i| Issue::from(&**i)).collect(),
            };
            let json_str =
                serde_json::to_string(&result).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            Ok::<String, anyhow::Error>(json_str)
        })
        .await
        .map_err(|e| JsError::new(&format!("Failed to spawn build task: {:?}", e)))?
        .map_or_else(
            |e| Err(JsError::new(&PrettyPrintError(&e).to_string())),
            |json_str| {
                js_sys::JSON::parse(&json_str)
                    .map_err(|e| JsError::new(&format!("Failed to parse JSON: {:?}", e)))
            },
        )
}

/// Subscribe to entrypoints changes.
/// This will watch for file changes and automatically rebuild, calling the callback with results.
/// Should be used for dev mode instead of manually calling build after file saves.
/// Returns a RootTask that must be held by JS to keep the subscription active.
pub async fn project_entrypoints_subscribe(
    config: Option<String>,
    callback: js_sys::Function,
) -> Result<RootTask, wasm_bindgen::JsError> {
    use wasm_bindgen::JsError;

    // First initialize the project in dev mode
    init_pack_project(config, true)
        .await
        .map_err(|e| JsError::new(&PrettyPrintError(&e).to_string()))?;

    let pack_project = GLOBAL_PACK_PROJECT
        .read()
        .as_ref()
        .cloned()
        .ok_or_else(|| JsError::new("pack project not initialized"))?;

    let turbo_tasks = pack_project.turbo_tasks.clone();
    let container = pack_project.container;

    let (tx, mut rx) = unbounded_channel::<String>();

    let turbo_tasks_clone = turbo_tasks.clone();
    let task_id = runtime()
        .spawn(async move {
            turbo_tasks_clone.spawn_root_task(move || {
                let tx = tx.clone();
                async move {
                    let start = Instant::now();
                    tracing::info!("dev build started...");

                    let entrypoints_with_issues_op =
                        get_all_written_entrypoints_with_issues_operation(container);
                    let entrypoints_with_issues = read_strongly_consistent_and_apply_effects(
                        entrypoints_with_issues_op,
                        |v| &v.effects,
                    )
                    .await?;
                    let EntrypointsWithIssues {
                        entrypoints: _,
                        issues,
                        effects: _,
                    } = &*entrypoints_with_issues;

                    tracing::info!("dev build finished in {:?}", start.elapsed());

                    let turbopack_result = TurbopackResult {
                        issues: issues.iter().map(|i| Issue::from(&**i)).collect(),
                    };

                    // Send result through channel
                    if let Ok(json_str) = serde_json::to_string(&turbopack_result) {
                        if let Err(e) = tx.send(json_str) {
                            tracing::error!("entrypointsSubscribe failed to send: {:?}", e);
                        }
                    }

                    // Return Completion to keep the task alive for re-execution
                    Ok(Completion::new())
                }
            })
        })
        .await
        .map_err(|e| JsError::new(&format!("Failed to spawn root task: {:?}", e)))?;

    // Spawn a receiver loop on the MAIN THREAD (local) to forward messages to JS callback.
    // We cannot use runtime().spawn() here because `callback` (js_sys::Function) is !Send.
    let turbo_tasks_for_receiver = turbo_tasks.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(json_str) = rx.recv().await {
            if let Ok(js_value) = js_sys::JSON::parse(&json_str) {
                let _ = callback.call1(&wasm_bindgen::JsValue::NULL, &js_value);
            }
        }
        drop(turbo_tasks_for_receiver); // Keep turbo_tasks alive until subscription ends
    });
    Ok(RootTask {
        turbo_tasks,
        task_id,
    })
}

/// Write all entrypoints to disk.
/// Should be called after receiving entrypoints from entrypointsSubscribe callback.
pub fn project_write_all_to_disk(callback: js_sys::Function) {
    wasm_bindgen_futures::spawn_local(async move {
        let pack_project = match GLOBAL_PACK_PROJECT.read().as_ref().cloned() {
            Some(p) => p,
            None => {
                tracing::warn!("writeAllToDisk: pack project not initialized");
                return;
            }
        };

        let turbo_tasks = pack_project.turbo_tasks.clone();
        let container = pack_project.container;

        let result = runtime()
            .spawn(async move {
                turbo_tasks
                    .run(async move {
                        // Use get_all_written_entrypoints_with_issues_operation to write output to disk
                        let entrypoints_with_issues_op =
                            get_all_written_entrypoints_with_issues_operation(container);
                        let entrypoints_with_issues = read_strongly_consistent_and_apply_effects(
                            entrypoints_with_issues_op,
                            |v| &v.effects,
                        )
                        .await?;
                        let EntrypointsWithIssues {
                            entrypoints: _,
                            issues,
                            effects: _,
                        } = &*entrypoints_with_issues;

                        Ok::<_, anyhow::Error>(issues.clone())
                    })
                    .await
            })
            .await;

        let result = match result {
            Ok(inner) => inner,
            Err(e) => {
                tracing::error!("writeAllToDisk spawn error: {:?}", e);
                return;
            }
        };

        match result {
            Ok(issues) => {
                let turbopack_result = TurbopackResult {
                    issues: issues.iter().map(|i| Issue::from(&**i)).collect(),
                };

                if let Ok(json_str) = serde_json::to_string(&turbopack_result) {
                    if let Ok(js_value) = js_sys::JSON::parse(&json_str) {
                        let _ = callback.call1(&wasm_bindgen::JsValue::NULL, &js_value);
                    }
                }
            }
            Err(e) => {
                tracing::error!("writeAllToDisk error: {:?}", e);
            }
        }
    });
}

pub async fn project_hmr_events(
    identifier: String,
    callback: js_sys::Function,
) -> Result<RootTask, wasm_bindgen::JsError> {
    use wasm_bindgen::JsError;

    let identifier: RcStr = identifier.into();

    let pack_project = GLOBAL_PACK_PROJECT
        .read()
        .as_ref()
        .cloned()
        .ok_or_else(|| JsError::new("pack project not initialized"))?;

    let turbo_tasks = pack_project.turbo_tasks.clone();
    let container = pack_project.container;
    let session = TransientInstance::new(());

    // Create channel for communication between root task and JS callback
    let (tx, mut rx) = unbounded_channel::<String>();

    // Spawn a root task that will be re-executed when dependencies change
    let turbo_tasks_clone = turbo_tasks.clone();
    let task_id = runtime()
        .spawn({
            let identifier = identifier.clone();
            let session = session.clone();
            async move {
                turbo_tasks_clone.spawn_root_task(move || {
                    let identifier = identifier.clone();
                    let session = session.clone();
                    let tx = tx.clone();
                    async move {
                        tracing::debug!("hmrSubscribe root task executing for {}", identifier);

                        let project = container.project().to_resolved().await?;
                        let state = project
                            .hmr_version_state(identifier.clone(), session)
                            .to_resolved()
                            .await?;

                        let update_op =
                            hmr_update_with_issues_operation(project, identifier.clone(), state);
                        let update =
                            read_strongly_consistent_and_apply_effects(update_op, |v| &v.effects)
                                .await?;
                        let HmrUpdateWithIssues {
                            update,
                            issues,
                            effects: _,
                        } = &*update;

                        // Update the version state (same as NAPI)
                        match &**update {
                            Update::Missing | Update::None => {}
                            Update::Total(TotalUpdate { to }) => {
                                state.set(to.clone()).await?;
                            }
                            Update::Partial(PartialUpdate { to, .. }) => {
                                state.set(to.clone()).await?;
                            }
                        }

                        // Convert issues to JSON (matching NAPI's NapiIssue format)
                        let issues_json: Vec<_> =
                            issues.iter().map(|issue| Issue::from(&**issue)).collect();

                        let result_json = match &**update {
                            Update::Missing => serde_json::json!({
                                "type": "notFound",
                                "resource": { "path": identifier.to_string() },
                                "issues": []
                            }),
                            Update::None => serde_json::json!({
                                "type": "issues",
                                "resource": { "path": identifier.to_string() },
                                "issues": issues_json
                            }),
                            Update::Total(_) => serde_json::json!({
                                "type": "restart",
                                "resource": { "path": identifier.to_string() },
                                "issues": issues_json
                            }),
                            Update::Partial(PartialUpdate { instruction, .. }) => {
                                serde_json::json!({
                                    "type": "partial",
                                    "resource": { "path": identifier.to_string() },
                                    "instruction": *instruction,
                                    "issues": issues_json
                                })
                            }
                        };

                        tracing::debug!("hmrSubscribe sending update for {}", identifier);
                        if let Ok(json_str) = serde_json::to_string(&result_json) {
                            let _ = tx.send(json_str);
                        }

                        Ok(Completion::new())
                    }
                })
            }
        })
        .await
        .map_err(|e| JsError::new(&format!("Failed to spawn root task: {:?}", e)))?;

    // Spawn receiver loop on main thread to forward messages to JS callback
    let turbo_tasks_for_receiver = turbo_tasks.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(json_str) = rx.recv().await {
            tracing::debug!("hmrSubscribe received update, calling JS callback");
            if let Ok(js_value) = js_sys::JSON::parse(&json_str) {
                let _ = callback.call1(&JsValue::NULL, &js_value);
            }
        }
        drop(turbo_tasks_for_receiver);
    });
    Ok(RootTask {
        turbo_tasks,
        task_id,
    })
}

/// Subscribe to compilation lifecycle events.
/// Emits "start" when computation begins, "end" when idle for aggregation_ms.
/// The callback receives { updateType: "start" | "end", value?: { duration: number, tasks: number } }
pub fn project_update_info_subscribe(aggregation_ms: u32, callback: js_sys::Function) {
    wasm_bindgen_futures::spawn_local(async move {
        let pack_project = match GLOBAL_PACK_PROJECT.read().as_ref().cloned() {
            Some(p) => p,
            None => {
                tracing::warn!("updateInfoSubscribe: pack project not initialized");
                return;
            }
        };

        let turbo_tasks = pack_project.turbo_tasks.clone();

        loop {
            // Wait for update info
            let turbo_tasks_clone = turbo_tasks.clone();
            let update_info = runtime()
                .spawn(async move {
                    turbo_tasks_clone
                        .aggregated_update_info(Duration::ZERO, Duration::ZERO)
                        .await
                })
                .await;

            // Emit start event
            let start_json = serde_json::json!({
                "updateType": "start",
                "value": null
            });
            if let Ok(js_value) =
                js_sys::JSON::parse(&serde_json::to_string(&start_json).unwrap_or_default())
            {
                let _ = callback.call1(&JsValue::NULL, &js_value);
            }

            // Get or wait for update info
            let update_info = match update_info {
                Ok(Some(info)) => info,
                _ => {
                    let turbo_tasks_clone = turbo_tasks.clone();
                    let aggregation_ms = aggregation_ms;
                    match runtime()
                        .spawn(async move {
                            turbo_tasks_clone
                                .get_or_wait_aggregated_update_info(Duration::from_millis(
                                    aggregation_ms.into(),
                                ))
                                .await
                        })
                        .await
                    {
                        Ok(info) => info,
                        Err(e) => {
                            tracing::error!("updateInfoSubscribe wait error: {:?}", e);
                            continue;
                        }
                    }
                }
            };

            // Emit end event with duration and tasks
            let end_json = serde_json::json!({
                "updateType": "end",
                "value": {
                    "duration": update_info.duration.as_millis() as u32,
                    "tasks": update_info.tasks as u32
                }
            });
            if let Ok(js_value) =
                js_sys::JSON::parse(&serde_json::to_string(&end_json).unwrap_or_default())
            {
                let _ = callback.call1(&JsValue::NULL, &js_value);
            }
        }
    });
}
