#![feature(future_join)]
#![feature(min_specialization)]

use clap::Parser;
use dunce::canonicalize;
use pack_api::project::{ProjectOptions, WatchOptions};
use serde_json::{Value, json};
use std::{cell::RefCell, fs::File, path::PathBuf, time::Instant};
use turbo_rcstr::RcStr;

use pack_core::tracing_presets::{
    TRACING_OVERVIEW_TARGETS, TRACING_TARGETS, TRACING_TURBO_TASKS_TARGETS,
    TRACING_TURBOPACK_TARGETS,
};
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt, util::SubscriberInitExt};
use turbo_tasks_malloc::TurboMalloc;
use turbopack_trace_utils::{
    exit::ExitGuard, filter_layer::FilterLayer, raw_trace::RawTraceLayer, trace_writer::TraceWriter,
};

use pack_cli::{Command, Mode, PartialProjectOptions, build, serve};

#[global_allocator]
static ALLOC: TurboMalloc = TurboMalloc;

fn main() {
    let args = Command::parse();

    let dev = matches!(args.mode, Mode::Dev);

    let watch = dev && args.watch.is_some_and(|watch| watch);

    let project_path: RcStr = canonicalize(args.project_dir)
        .unwrap()
        .to_str()
        .unwrap()
        .into();

    let root_dir = args.root_dir.map(RcStr::from);

    thread_local! {
        static LAST_SWC_ATOM_GC_TIME: RefCell<Option<Instant>> = const { RefCell::new(None) };
    }

    tokio::runtime::Builder::new_multi_thread()
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
        .disable_lifo_slot()
        .build()
        .unwrap()
        .block_on(async move {
            let trace = std::env::var("TURBOPACK_TRACING").ok();

            let _guard = if let Some(mut trace) = trace.filter(|v| !v.is_empty()) {
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

                let subscriber = subscriber.with(FilterLayer::try_new(&trace).unwrap());
                let trace_file = PathBuf::from(&project_path).join("trace.log");
                let trace_writer = File::create(trace_file).unwrap();
                let (trace_writer, guard) = TraceWriter::new(trace_writer);
                let subscriber = subscriber.with(RawTraceLayer::new(trace_writer));

                let guard = ExitGuard::new(guard).unwrap();

                subscriber.init();

                Some(guard)
            } else {
                tracing_subscriber::fmt()
                    .with_env_filter(
                        EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| EnvFilter::new("pack_cli=info,pack_api=info")),
                    )
                    .with_timer(tracing_subscriber::fmt::time::SystemTime)
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
                    .init();

                None
            };

            let project_options_path = PathBuf::from(&project_path).join("project_options.json");
            let mut project_options_file = File::open(&project_options_path)
                .unwrap_or_else(|_| panic!("failed to load {}", project_options_path.display()));

            let mut partial_project_options: PartialProjectOptions =
                serde_json::from_reader(&mut project_options_file).unwrap();
            let mode = if dev { "development" } else { "production" };

            partial_project_options.config = partial_project_options.config.as_mut().map_or(
                Some(json!(format!(r#"{{ "mode": {mode}}}"#,))),
                |mut config| {
                    if let Value::Object(map) = &mut config {
                        map.insert("mode".to_string(), mode.into());
                    }
                    Some(config.take())
                },
            );
            let project_options = ProjectOptions {
                root_path: root_dir
                    .as_ref()
                    .map(|r| {
                        canonicalize(PathBuf::from(&r))
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .into()
                    })
                    .or(partial_project_options.root_path.as_ref().map(|r| {
                        canonicalize(PathBuf::from(&project_path).join(r))
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .into()
                    }))
                    .unwrap_or_else(|| project_path.clone()),
                project_path,
                config: partial_project_options.config.unwrap().to_string().into(),
                process_env: partial_project_options.process_env.unwrap_or_default(),
                define_env: partial_project_options.define_env.unwrap_or_default(),

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
        })
        .unwrap();
}
