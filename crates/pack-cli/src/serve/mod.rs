use std::{
    env::current_dir,
    future::join,
    io::{Write, stdout},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use owo_colors::OwoColorize;
use pack_api::project::ProjectOptions;
use turbo_tasks::{
    TransientInstance, UpdateInfo, Vc,
    util::{FormatBytes, FormatDuration},
};
use turbo_tasks_malloc::TurboMalloc;
use turbopack_cli_utils::issue::{ConsoleUi, LogOptions};
use turbopack_core::issue::IssueSeverity;
use turbopack_dev_server::{DevServer, DevServerBuilder};

use crate::initialize_project_container;

pub mod source;
use crate::serve::source::{ServerSourceProvider, create_web_entry_source};

pub async fn run(options: ProjectOptions) -> Result<()> {
    let dev = options.dev;

    tracing::info!("bundling with watching",);

    let start = Instant::now();

    let (turbo_tasks, project_container) = initialize_project_container(options, dev).await?;

    tracing::debug!("turbo task initialized");

    turbo_tasks
        .clone()
        .run_once(async move {
            let project = project_container.project().to_resolved().await?;

            let web_source = create_web_entry_source(*project).to_resolved().await?;

            let serve_source_provider = ServerSourceProvider { web_source };

            let project_dir = PathBuf::from(project.await?.project_path.clone());
            let issue_reporter_arc = Arc::new(move || {
                let project_dir = project_dir.clone();
                Vc::upcast(ConsoleUi::new(TransientInstance::new(LogOptions {
                    current_dir: current_dir().unwrap(),
                    project_dir,
                    show_all: true,
                    log_detail: true,
                    log_level: IssueSeverity::Note,
                })))
            });

            let server_builder = dev_server_builder().await?;

            let server = server_builder.serve(
                turbo_tasks.clone(),
                serve_source_provider,
                issue_reporter_arc,
            );

            let stats_future = async move {
                tracing::info!(
                    "{event_type} - initial compilation {start} ({memory})",
                    event_type = "event".purple(),
                    start = FormatDuration(start.elapsed()),
                    memory = FormatBytes(TurboMalloc::memory_usage())
                );

                let mut progress_counter = 0;
                loop {
                    let update_future = turbo_tasks
                        .aggregated_update_info(Duration::from_millis(100), Duration::MAX);

                    if let Some(UpdateInfo {
                        duration,
                        tasks,
                        reasons,
                        ..
                    }) = update_future.await
                    {
                        progress_counter = 0;
                        match !reasons.is_empty() {
                            true => {
                                tracing::warn!(
                            "\x1b[2K{event_type} - {reasons} {duration} ({tasks} tasks, {memory})",
                            event_type = "event".purple(),
                            duration = FormatDuration(duration),
                            tasks = tasks,
                            memory = FormatBytes(TurboMalloc::memory_usage())
                        );
                            }
                            false => {
                                tracing::info!(
                                    "{event_type} - compilation {duration} ({tasks} tasks, \
                             {memory})",
                                    event_type = "event".purple(),
                                    duration = FormatDuration(duration),
                                    tasks = tasks,
                                    memory = FormatBytes(TurboMalloc::memory_usage())
                                );
                            }
                        }
                    } else {
                        progress_counter += 1;
                        tracing::info!(
                    "\x1b[2K{event_type} - updating for {progress_counter}s... ({memory})\r",
                    event_type = "event".purple(),
                    memory = FormatBytes(TurboMalloc::memory_usage())
                );

                        let _ = stdout().lock().flush();
                    }
                }
            };

            join!(stats_future, async { server.future.await.unwrap() }).await;

            Ok(())
        })
        .await
}

async fn dev_server_builder() -> Result<DevServerBuilder> {
    // max_attempts of 1 means we loop 0 times.
    let max_attempts = 9;
    let mut attempts = 0;
    let host = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let port = 3000;
    loop {
        let current_port = port + attempts;
        let addr = SocketAddr::new(host, current_port);
        let listen_result = DevServer::listen(addr);

        if let Err(e) = &listen_result
            && attempts < max_attempts
        {
            // Returned error from `listen` is not `std::io::Error` but `anyhow::Error`,
            // so we need to access its source to check if it is
            // `std::io::ErrorKind::AddrInUse`.
            let should_retry = e
                .source()
                .and_then(|e| {
                    e.downcast_ref::<std::io::Error>()
                        .map(|e| e.kind() == std::io::ErrorKind::AddrInUse)
                })
                .unwrap_or(false);

            if should_retry {
                tracing::warn!(
                    "{} - Port {} is in use, trying {} instead",
                    "warn ".yellow(),
                    current_port,
                    current_port + 1
                );
                attempts += 1;
                continue;
            }
        }

        tracing::info!("listenining on http://{}:{}", host, current_port);

        return listen_result;
    }
}
