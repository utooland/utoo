#![feature(future_join)]
#![feature(arbitrary_self_types)]
#![feature(arbitrary_self_types_pointers)]
#![allow(unexpected_cfgs)]

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use pack_api::project::{ProjectContainer, ProjectOptions, WatchOptions};
use serde::Deserialize;
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, TurboTasks};
use turbo_tasks_backend::{BackendOptions, TurboTasksBackend, noop_backing_storage};

pub mod build;
pub mod serve;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Command {
    #[arg(short, long)]
    pub mode: Mode,

    #[arg(short, long)]
    pub watch: Option<bool>,

    #[arg(short, long)]
    pub project_path: Option<String>,

    #[arg(short, long)]
    pub root_path: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum Mode {
    Build,
    Dev,
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PartialProjectOptions {
    /// A map of environment variables to use when compiling code.
    pub process_env: Option<Vec<(RcStr, RcStr)>>,

    /// Filesystem watcher options.
    pub watch: Option<WatchOptions>,

    /// The build id.
    pub build_id: Option<RcStr>,

    /// Absolute path for `@utoo/pack`.
    pub pack_path: Option<RcStr>,

    #[serde(flatten)]
    pub config: serde_json::Value,
}

pub async fn initialize_project_container(
    options: ProjectOptions,
    dev: bool,
) -> Result<
    (
        Arc<TurboTasks<TurboTasksBackend>>,
        ResolvedVc<ProjectContainer>,
    ),
    anyhow::Error,
> {
    let turbo_tasks = TurboTasks::new(TurboTasksBackend::new(
        BackendOptions {
            dependency_tracking: dev,
            storage_mode: None,
            ..Default::default()
        },
        noop_backing_storage(),
    ));
    let project_container = turbo_tasks
        .run(async move {
            let container_op = ProjectContainer::new_operation("utoopack-cli".into(), dev);
            ProjectContainer::initialize(container_op, options).await?;
            container_op
                .resolve()
                .strongly_consistent()
                .await
                .context("failed to create project container")
        })
        .await?;

    Ok((turbo_tasks, project_container))
}
