#![feature(future_join)]
#![feature(min_specialization)]
#![feature(arbitrary_self_types)]
#![feature(arbitrary_self_types_pointers)]
#![allow(unexpected_cfgs)]

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use pack_api::project::{DefineEnv, ProjectContainer, ProjectOptions, WatchOptions};
use serde::{Deserialize, Serialize};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, TurboTasks};
use turbo_tasks_backend::{
    BackendOptions, NoopBackingStorage, TurboTasksBackend, noop_backing_storage,
};

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
    pub project_dir: String,

    #[arg(short, long)]
    pub root_dir: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum Mode {
    Build,
    Dev,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PartialProjectOptions {
    /// A root path from which all files must be nested under. Trying to access
    /// a file outside this root will fail. Think of this as a chroot.
    pub root_path: Option<RcStr>,

    /// A path inside the root_path which contains the app/pages directories.
    pub project_path: Option<RcStr>,

    /// The contents of next.config.js, serialized to JSON.
    pub config: Option<serde_json::Value>,

    /// A map of environment variables to use when compiling code.
    pub process_env: Option<Vec<(RcStr, RcStr)>>,

    /// A map of environment variables which should get injected at compile
    /// time.
    pub define_env: Option<DefineEnv>,

    /// Filesystem watcher options.
    pub watch: Option<WatchOptions>,

    /// The build id.
    pub build_id: Option<RcStr>,
}

pub async fn initialize_project_container(
    options: ProjectOptions,
    dev: bool,
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
            let project_container = ProjectContainer::new("utoo-pack-cli".into(), dev);
            let project_container = project_container.to_resolved().await?;
            project_container.initialize(options).await?;
            Ok(project_container)
        })
        .await?;

    Ok((turbo_tasks, project_container))
}
