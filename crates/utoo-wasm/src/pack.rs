use std::sync::Arc;
use wasmtimer::std::Instant;

use anyhow::Result;
use pack_api::{
    entrypoint::{get_all_written_entrypoints_with_issues_operation, EntrypointsWithIssues},
    project::{ProjectContainer, ProjectOptions, WatchOptions},
};
use serde::{Deserialize, Serialize};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, TurboTasks};
use turbo_tasks_backend::{
    noop_backing_storage, BackendOptions, NoopBackingStorage, TurboTasksBackend,
};
use turbo_tasks_malloc::TurboMalloc;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

pub fn register() {
    pack_api::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
#[wasm_bindgen]
pub struct PartialProjectOptions {
    #[wasm_bindgen(getter_with_clone)]
    pub project_path: String,

    #[wasm_bindgen(getter_with_clone)]
    pub config: Option<String>,
}

#[wasm_bindgen]
pub async fn build(partial_options: PartialProjectOptions) -> std::result::Result<(), JsValue> {
    let project_path: RcStr = partial_options.project_path.into();
    let config: RcStr = partial_options.config.unwrap_or("{}".to_string()).into();
    let options = ProjectOptions {
        root_path: project_path.clone(),
        project_path: project_path.clone(),
        config,
        process_env: Default::default(),
        define_env: Default::default(),
        watch: WatchOptions {
            enable: false,
            ..Default::default()
        },
        dev: false,
        build_id: project_path.clone(),
    };

    tracing::info!("Bundle with options: {options:#?}");

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    rt.spawn(async move { build_internal(options).await })
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(())
}

async fn build_internal(options: ProjectOptions) -> Result<()> {
    tracing::info!("bundling",);

    let start = Instant::now();

    let (turbo_tasks, project_container) = initialize_project_container(options).await?;

    let (entrypoints, _issues, _diagnostics) = turbo_tasks
        .run_once(async move {
            let entrypoints_with_issues_op =
                get_all_written_entrypoints_with_issues_operation(project_container);

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

    let memory = TurboMalloc::memory_usage();
    tracing::info!("memory usage: {} MiB", memory / 1024 / 1024);

    let start = Instant::now();
    drop(turbo_tasks);

    tracing::info!("drop {:?}", start.elapsed());

    Ok(())
}

async fn initialize_project_container(
    options: ProjectOptions,
) -> Result<
    (
        Arc<TurboTasks<TurboTasksBackend<NoopBackingStorage>>>,
        ResolvedVc<ProjectContainer>,
    ),
    anyhow::Error,
> {
    let turbo_tasks = TurboTasks::new(TurboTasksBackend::new(
        BackendOptions {
            dependency_tracking: true,
            storage_mode: None,
            ..Default::default()
        },
        noop_backing_storage(),
    ));
    let project_container = turbo_tasks
        .run_once(async move {
            let project_container = ProjectContainer::new("utoopack-web".into(), false);
            let project_container = project_container.to_resolved().await?;
            project_container.initialize(options).await?;
            Ok(project_container)
        })
        .await?;

    Ok((turbo_tasks, project_container))
}
