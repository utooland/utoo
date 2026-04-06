use clap::Parser;
use dunce::canonicalize;
use pack_api::project::{ProjectOptions, WatchOptions};
use serde_json::Value;
use std::{cell::RefCell, fs::File, path::PathBuf, time::Instant};
use turbo_rcstr::RcStr;

use pack_core::tracing_presets::{
    TRACING_OVERVIEW_TARGETS, TRACING_TARGETS, TRACING_TURBO_TASKS_TARGETS,
    TRACING_TURBOPACK_TARGETS,
};
use tracing_subscriber::EnvFilter;
use turbo_tasks_malloc::TurboMalloc;

use pack_cli::{Command, Mode, PartialProjectOptions, build, serve};

#[global_allocator]
static ALLOC: TurboMalloc = TurboMalloc;

fn main() {
    let args = Command::parse();

    let dev = matches!(args.mode, Mode::Dev);

    let watch = dev && args.watch.is_some_and(|watch| watch);

    let project_path: RcStr = canonicalize(args.project_path.unwrap_or_else(|| ".".to_string()))
        .unwrap()
        .to_string_lossy()
        .into();

    let root_path = args.root_path.map(RcStr::from);

    thread_local! {
        static LAST_SWC_ATOM_GC_TIME: RefCell<Option<Instant>> = const { RefCell::new(None) };
    }

    #[cfg(feature = "dial9")]
    let dial9_trace_path = PathBuf::from(&project_path).join("./.trace/dial9_trace.bin");

    let future = async move {
        let trace = std::env::var("TURBOPACK_TRACING").ok();

        if let Some(mut trace) = trace.filter(|v| !v.is_empty()) {
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

            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::try_new(&trace).unwrap())
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| EnvFilter::new("pack_cli=info,pack_api=info")),
                )
                .with_target(false)
                .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NONE)
                .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
                    "%Y-%m-%d %H:%M:%S.%3f".to_string(),
                ))
                .init();
        };

        let project_options_path = PathBuf::from(&project_path).join("utoopack.json");
        let mut project_options_file = File::open(&project_options_path)
            .unwrap_or_else(|_| panic!("failed to load {}", project_options_path.display()));

        let mut partial_project_options: PartialProjectOptions =
            serde_json::from_reader(&mut project_options_file).unwrap();
        let mode = if dev { "development" } else { "production" };

        if let Value::Object(map) = &mut partial_project_options.config {
            map.insert("mode".to_string(), mode.into());
        }

        let project_options = ProjectOptions {
            root_path: root_path
                .as_ref()
                .map(|r| {
                    canonicalize(PathBuf::from(r.as_str()))
                        .unwrap()
                        .to_string_lossy()
                        .into()
                })
                .unwrap_or_else(|| project_path.clone()),
            project_path,
            config: partial_project_options.config.to_string().into(),
            process_env: partial_project_options.process_env.unwrap_or_default(),

            watch: WatchOptions {
                enable: watch,
                ..Default::default()
            },
            build_id: partial_project_options.build_id.unwrap_or_default(),
            dev,
            pack_path: partial_project_options.pack_path.unwrap_or_default(),
        };

        if watch {
            serve::run(project_options).await
        } else {
            build::run(project_options).await
        }
    };

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder
        .enable_all()
        .on_thread_stop(|| {
            TurboMalloc::thread_stop();
        })
        .on_thread_park(|| {
            LAST_SWC_ATOM_GC_TIME.with_borrow_mut(|cell| {
                use std::time::Duration;

                if cell.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                    swc_core::ecma::atoms::hstr::global_atom_store_gc();
                    *cell = Some(Instant::now());
                }
            });
        })
        .disable_lifo_slot();

    #[cfg(not(feature = "dial9"))]
    {
        let runtime = builder.build().unwrap();
        runtime.block_on(future).unwrap();
    }

    #[cfg(feature = "dial9")]
    {
        use dial9_tokio_telemetry::telemetry::{RotatingWriter, TracedRuntime};
        let writer = RotatingWriter::builder()
            .base_path(dial9_trace_path)
            .max_file_size(100 * 1024 * 1024)
            .max_total_size(500 * 1024 * 1024)
            .build()
            .unwrap();

        let (runtime, _guard) = TracedRuntime::build_and_start(builder, writer).unwrap();
        runtime.block_on(future).unwrap();
    }
}
