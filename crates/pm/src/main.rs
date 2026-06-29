use std::process;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Commands, detect_shell_from_env};
use crate::cmd::clean::clean;
use crate::cmd::list::list_dependencies;
use crate::cmd::rebuild::rebuild;
use crate::cmd::run::{run, run_fallback};
use crate::cmd::update::update;
use crate::cmd::view::view;
use crate::constants::{APP_NAME, APP_VERSION};
use crate::helper::auto_update::init_auto_update;
use crate::service::script::{MissingScript, ScriptExit};
use crate::service::workspace::WorkspaceFilter;
use crate::util::cli_enum::{ConfigScope, ScriptPolicy};
use crate::util::logger::{get_log_file_path, init_tracing, log_time, log_time_end};
use crate::util::user_config::{
    init_registry, set_cache_dir, set_legacy_peer_deps, set_manifests_concurrency_limit,
};

mod cli;
mod cmd;
mod constants;
mod fs;
mod helper;
mod model;
mod service;
mod util;

fn main() {
    crate::util::sysconf::init();

    let worker_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(worker_threads)
        .build()
        .expect("failed to build tokio runtime")
        .block_on(async_main());

    if let Err(e) = result {
        // A failed package script propagates its own exit status so
        // `utoo run <script>` mirrors the script: a non-zero `exit N` becomes
        // N, and a signal death (e.g. SIGPIPE from `script | head`) becomes
        // 128+N. Any other error keeps the generic exit code 1.
        let exit_code = e.downcast_ref::<ScriptExit>().map_or(1, |s| s.code);
        if let Some(chain) = util::format_print::format_resolve_chain(&e) {
            tracing::error!("{:#}\n\n{chain}", e);
        } else {
            tracing::error!("{:#}", e);
        }
        if let Some(log_path) = get_log_file_path() {
            eprintln!("Full logs saved to: {}", log_path.display());
        }
        process::exit(exit_code);
    }
}

async fn async_main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Check for help flag
    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        let config = crate::util::config_file::Config::load(ConfigScope::Local).await?;
        let config_service = crate::service::config::ConfigService::new(config);
        config_service.print_help()?;
        return Ok(());
    }

    log_time(); // Start global timer
    let cli = Cli::parse();

    // Check for version flag
    if cli.version {
        println!("{APP_VERSION}");
        return Ok(());
    }

    // `utx --version` (= `utoo x --version`): the version flag is disabled at
    // the top level, so a leading `--version`/`-v` lands in the Execute
    // `command` (see its doc) instead of being rejected. Handle it here —
    // before registry selection and auto-update — so it behaves like `npx
    // --version` rather than being treated as a package to run. (`--help`/`-h`
    // is intercepted by clap's built-in help for the `x` subcommand.)
    if let Some(Commands::Execute { command, .. }) = &cli.command
        && matches!(command.as_str(), "--version" | "-v")
    {
        println!("{APP_VERSION}");
        return Ok(());
    }

    // Handle completions early to avoid unnecessary initialization (tracing, registry, auto-update)
    if let Some(Commands::Completions { shell }) = cli.command {
        let shell = shell.or_else(detect_shell_from_env);

        let Some(shell) = shell else {
            eprintln!(
                "Could not detect shell. Usage: utoo completions <bash|zsh|fish|powershell|elvish>"
            );
            process::exit(2);
        };

        tokio::task::spawn_blocking(move || {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, APP_NAME, &mut std::io::stdout());
        })
        .await
        .context("Failed to generate shell completions")?;

        return Ok(());
    }

    // Initialize tracing (replaces set_verbose)
    let (log_file, _guard) = init_tracing(cli.verbose).context("Failed to initialize logging")?;

    tracing::debug!(
        log_file = %log_file.display(),
        verbose = cli.verbose,
        "Logger initialized"
    );

    // Run --from migration early, before config is cached by init_registry
    if let Some(Commands::Install(args)) = &cli.command {
        cmd::install::migrate_from(args.from).await?;
    }

    // global registry
    init_registry(cli.registry).await?;

    // set cache directory
    set_cache_dir(cli.cache_dir).await;

    // set legacy_peer_deps when set --legacy
    if cli.legacy_peer_deps == Some(true) {
        set_legacy_peer_deps(cli.legacy_peer_deps);
    }

    // set manifests concurrency limit if specified
    if cli.manifests_concurrency_limit.is_some() {
        set_manifests_concurrency_limit(cli.manifests_concurrency_limit);
    }

    // Auto update: check cache → update or refresh in background
    init_auto_update().await;

    match cli.command {
        Some(Commands::Clean { pattern }) => {
            clean(&pattern).await?;
            log_time_end(&format!("{pattern} cleaned"));
        }
        Some(Commands::Install(args)) => {
            cmd::install::run(args, cli.legacy_peer_deps).await?;
        }
        Some(Commands::Uninstall {
            specs,
            workspace,
            ignore_scripts,
        }) => {
            cmd::install::uninstall(specs, workspace, ScriptPolicy::from(ignore_scripts)).await?;
        }
        Some(Commands::Rebuild) => {
            let cwd = std::env::current_dir()?;
            rebuild(&cwd).await?;
            log_time_end("All packages rebuilt");
        }
        Some(Commands::Deps { workspace_only }) => {
            cmd::deps::run(workspace_only).await?;
        }
        Some(Commands::Update) => {
            update(ScriptPolicy::Run).await?;
            log_time_end("All packages updated");
        }
        Some(Commands::List { package }) => {
            let cwd = std::env::current_dir()?;
            list_dependencies(&cwd, &package).await?;
        }
        Some(Commands::Execute { command, args }) => {
            service::execute::execute_package(&command, args).await?;
        }
        Some(Commands::Run {
            script,
            workspace,
            workspaces,
            if_present,
            args,
        }) => {
            let missing = if if_present {
                MissingScript::Skip
            } else {
                MissingScript::Fail
            };
            run(
                script.as_deref(),
                WorkspaceFilter::from_flags(workspace, workspaces),
                missing,
                (!args.is_empty()).then_some(args),
            )
            .await?;
        }
        Some(Commands::View { package }) => {
            view(&package).await?;
        }
        Some(Commands::Link { packages, prefix }) => {
            cmd::link::run(packages, prefix).await?;
        }
        Some(Commands::Init { yes }) => {
            service::init::init(yes, None).await?;
            log_time_end("package.json created");
        }
        Some(Commands::Pack { path, dry_run }) => {
            cmd::pm_pack::pack(path, dry_run.into()).await?;
            log_time_end("Pack complete");
        }
        Some(Commands::Publish { tag, dry_run, otp }) => {
            // `--workspace`/`--filter` selects member(s); empty means the current
            // package. `--workspaces` is intentionally NOT honored here to avoid
            // an accidental publish of every member.
            let filter = if cli.workspace.is_empty() {
                WorkspaceFilter::Current
            } else {
                WorkspaceFilter::Selected(cli.workspace)
            };
            cmd::publish::publish(tag.as_deref(), dry_run.into(), otp.as_deref(), filter).await?;
        }
        Some(Commands::Ping { registry }) => {
            cmd::ping::ping(registry.as_deref()).await?;
        }
        Some(Commands::Login) => {
            cmd::login::login().await?;
        }
        Some(Commands::Whoami) => {
            cmd::whoami::whoami().await?;
        }
        Some(Commands::Logout) => {
            cmd::logout::logout().await?;
        }
        Some(Commands::Config { command }) => {
            cmd::config::run(command).await?;
        }
        None => match cli.script_name {
            // A bare `utoo <name>`: custom command from config, else script
            Some(script_name) => {
                run_fallback(
                    &script_name,
                    WorkspaceFilter::from_flags(cli.workspace, cli.workspaces),
                    cli.script_args,
                )
                .await?;
            }
            // Default to install if no arguments
            None => cmd::install::install_cwd(ScriptPolicy::from(cli.ignore_scripts)).await?,
        },
        // Completions is handled early before initialization
        Some(Commands::Completions { .. }) => unreachable!(),
    }

    Ok(())
}
