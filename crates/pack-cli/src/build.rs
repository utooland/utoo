use std::time::Instant;

use anyhow::Result;
use pack_api::{
    entrypoint::EntrypointsWithIssues,
    entrypoint::get_all_written_entrypoints_with_issues_operation, project::ProjectOptions,
};
use turbo_tasks::read_strongly_consistent_and_apply_effects;
use turbo_tasks_malloc::TurboMalloc;

use crate::initialize_project_container;

pub async fn run(options: ProjectOptions) -> Result<()> {
    let dev = options.dev;

    tracing::info!(
        "bundling with {} mode",
        if dev { "development" } else { "production" }
    );

    let start = Instant::now();

    let (turbo_tasks, project_container) = initialize_project_container(options, dev).await?;

    let (_entrypoints, _issues) = turbo_tasks
        .run(async move {
            let entrypoints_with_issues_op =
                get_all_written_entrypoints_with_issues_operation(project_container);

            let entrypoints_with_issues =
                read_strongly_consistent_and_apply_effects(entrypoints_with_issues_op, |v| {
                    &v.effects
                })
                .await?;
            let EntrypointsWithIssues {
                entrypoints,
                issues,
                effects: _,
            } = &*entrypoints_with_issues;

            Ok((entrypoints.clone(), issues.clone()))
        })
        .await?;

    tracing::info!("all project entrypoints wrote to disk.");

    tracing::info!("pack tasks finished in {:?}", start.elapsed());

    let memory = TurboMalloc::memory_usage();
    tracing::info!("memory usage: {} MiB", memory / 1024 / 1024);

    let start = Instant::now();
    drop(turbo_tasks);

    tracing::info!("drop {:?}", start.elapsed());

    Ok(())
}
