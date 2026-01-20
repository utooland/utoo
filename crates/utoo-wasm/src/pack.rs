use anyhow::{Context, Result};
use pack_api::{
    endpoint::Endpoint,
    entrypoint::{get_all_written_entrypoints_with_issues_operation, EntrypointsWithIssues},
    project::{ProjectContainer, ProjectOptions, WatchOptions},
    tasks::BundlerTurboTasks,
    utils::StyledStringSerialize,
};
use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::{ops::Deref, path::PathBuf, str::FromStr, sync::Arc};
use tokio::time::Instant;
use turbo_rcstr::{rcstr, RcStr};
use turbo_tasks::{OperationVc, ReadConsistency, ResolvedVc, TurboTasks};
use turbo_tasks_backend::{
    noop_backing_storage, BackendOptions, NoopBackingStorage, TurboTasksBackend,
};
use turbo_tasks_fs::FileContent;
use turbopack_core::{
    diagnostics::PlainDiagnostic,
    error::PrettyPrintError,
    issue::{PlainIssue, PlainIssueSource, PlainSource},
    source_pos::SourcePos as SourcePosInner,
};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::fs::Fs;
use crate::tokio_runtime::TOKIO_RUNTIME;

unsafe extern "C" {
    pub fn __wasm_call_ctors();
}

/// Global pack project instance
static GLOBAL_PACK_PROJECT: RwLock<Option<Arc<PackProject>>> = RwLock::new(None);

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
pub struct PartialProjectOptions {
    pub project_path: String,

    pub config: Option<String>,
}

pub struct PackProject {
    pub turbo_tasks: BundlerTurboTasks,
    pub container: ResolvedVc<ProjectContainer>,
}

impl PackProject {
    pub async fn initialize(options: ProjectOptions) -> Result<Self> {
        let turbo_tasks = create_turbo_tasks()?;
        let container = turbo_tasks
            .run_once(async move {
                let project_container = ProjectContainer::new("utoopack-web".into(), false);
                let project_container = project_container.to_resolved().await?;
                project_container.initialize(options).await?;
                Ok(project_container)
            })
            .await?;

        Ok(PackProject {
            turbo_tasks,
            container,
        })
    }

    pub async fn build(&self) -> Result<TurbopackResult> {
        let start = Instant::now();
        let turbo_tasks = self.turbo_tasks.clone();
        let container = self.container;
        let (entrypoints, issues, diags) = turbo_tasks
            .run_once(async move {
                let entrypoints_with_issues_op =
                    get_all_written_entrypoints_with_issues_operation(container);

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
            })
            .await?;

        tracing::info!("all project entrypoints wrote to disk.");

        tracing::info!(
            "pack tasks with {} apps {} libraries finished in {:?}",
            entrypoints
                .apps
                .as_ref()
                .map(|apps| apps.0.len())
                .unwrap_or_default(),
            entrypoints
                .libraries
                .as_ref()
                .map(|libraries| libraries.0.len())
                .unwrap_or_default(),
            start.elapsed()
        );

        Ok(TurbopackResult {
            issues: issues.iter().map(|i| Issue::from(&**i)).collect(),
            diagnostics: diags.iter().map(|d| Diagnostic::from(d)).collect(),
        })
    }
}

pub fn create_turbo_tasks() -> Result<BundlerTurboTasks> {
    Ok(BundlerTurboTasks::Memory(TurboTasks::new(
        turbo_tasks_backend::TurboTasksBackend::new(
            turbo_tasks_backend::BackendOptions {
                storage_mode: None,
                dependency_tracking: true,
                ..Default::default()
            },
            noop_backing_storage(),
        ),
    )))
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
pub struct Diagnostic {
    pub category: String,
    pub name: String,
    pub payload: FxHashMap<String, String>,
}

impl Diagnostic {
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

#[derive(Serialize, Deserialize)]
pub struct TurbopackResult {
    pub issues: Vec<Issue>,
    pub diagnostics: Vec<Diagnostic>,
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
/// This will clean up any previous turbo-tasks before creating a new project.
pub async fn init_pack_project(dev_mode: bool) -> Result<()> {
    // Clean up previous turbo-tasks and reset the project
    {
        let mut pack_project_guard = GLOBAL_PACK_PROJECT.write();
        if let Some(old_project) = pack_project_guard.take() {
            // Drop the write guard before stopping turbo-tasks to avoid deadlock
            drop(pack_project_guard);
            old_project.turbo_tasks.stop_and_wait().await;
        }
    }

    let cwd = opfs_project::get_cwd().to_string_lossy().to_string();
    let project_root = if cwd.starts_with('/') {
        cwd
    } else {
        tokio_fs_ext::current_dir()?
            .join(cwd)
            .to_string_lossy()
            .to_string()
    };

    let config_path = std::path::PathBuf::from(&project_root)
        .join("utoopack.json")
        .to_string_lossy()
        .to_string();

    let config = Fs::read_to_string(&config_path).await.ok();

    let partial_options = PartialProjectOptions {
        project_path: project_root,
        config,
    };
    let project_path: RcStr = partial_options.project_path.into();

    // Parse config JSON and inject mode based on dev_mode
    let config_str = partial_options.config.unwrap_or("{}".to_string());
    let mut config_json: serde_json::Value =
        serde_json::from_str(&config_str).unwrap_or(serde_json::json!({}));

    // Set mode based on dev_mode flag
    config_json["mode"] = serde_json::json!(if dev_mode {
        "development"
    } else {
        "production"
    });

    let config: RcStr = serde_json::to_string(&config_json)
        .unwrap_or("{}".to_string())
        .into();

    let options = ProjectOptions {
        root_path: project_path.clone(),
        project_path: project_path.clone(),
        config,
        build_id: project_path.clone(),
        watch: WatchOptions {
            enable: dev_mode,
            ..Default::default()
        },
        define_env: Default::default(),
        dev: dev_mode,
        pack_path: rcstr!("./"),
        process_env: Default::default(),
    };

    tracing::info!("[pack] ProjectOptions: {:?}", options);

    let rt = TOKIO_RUNTIME
        .get()
        .ok_or_else(|| anyhow::anyhow!("tokio runtime not initialized"))?;
    let pack_context = rt
        .spawn(PackProject::initialize(options))
        .await
        .context("fail to initialize pack project")??;

    let mut pack_project_guard = GLOBAL_PACK_PROJECT.write();
    *pack_project_guard = Some(Arc::new(pack_context));

    Ok(())
}

/// Run pack with the current pack project.
/// Returns the build result as a TurbopackResult.
pub async fn run_pack() -> Result<TurbopackResult> {
    let pack_project = GLOBAL_PACK_PROJECT
        .read()
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("pack project not initialized"))?;

    let rt = TOKIO_RUNTIME
        .get()
        .ok_or_else(|| anyhow::anyhow!("tokio runtime not initialized"))?;

    rt.spawn(async move { pack_project.build().await })
        .await
        .context("failed to spawn build task")?
}

/// WASM API for build operation.
pub async fn build() -> std::result::Result<wasm_bindgen::JsValue, wasm_bindgen::JsError> {
    use wasm_bindgen::JsError;

    init_pack_project(false)
        .await
        .map_err(|e| JsError::new(&PrettyPrintError(&e).to_string()))?;

    run_pack().await.map_or_else(
        |e| Err(JsError::new(&PrettyPrintError(&e).to_string())),
        |result| {
            let json_str =
                serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))?;
            js_sys::JSON::parse(&json_str)
                .map_err(|e| JsError::new(&format!("Failed to parse JSON: {:?}", e)))
        },
    )
}

/// WASM API for dev operation.
pub async fn dev() -> std::result::Result<wasm_bindgen::JsValue, wasm_bindgen::JsError> {
    use wasm_bindgen::JsError;

    init_pack_project(true)
        .await
        .map_err(|e| JsError::new(&PrettyPrintError(&e).to_string()))?;

    run_pack().await.map_or_else(
        |e| Err(JsError::new(&PrettyPrintError(&e).to_string())),
        |result| {
            let json_str =
                serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))?;
            js_sys::JSON::parse(&json_str)
                .map_err(|e| JsError::new(&format!("Failed to parse JSON: {:?}", e)))
        },
    )
}
