use std::{io, process};

use anyhow::{Context, Result};
use clap::error::ErrorKind as ClapErrorKind;
use clap::{CommandFactory, FromArgMatches};

use crate::cli::{Cli, Commands, detect_shell_from_env};
use crate::cmd::clean::clean;
use crate::cmd::list::list_dependencies;
use crate::cmd::rebuild::rebuild;
use crate::cmd::run::{run, run_fallback};
use crate::cmd::update::update;
use crate::cmd::view::view;
use crate::constants::{APP_NAME, APP_VERSION};
use crate::error::{CliError, ErrorKind, classify};
use crate::helper::auto_update::init_auto_update;
use crate::service::config::ConfigService;
use crate::service::script::{MissingScript, ScriptExit};
use crate::service::workspace::WorkspaceFilter;
use crate::util::cli_enum::{ConfigScope, ScriptPolicy};
use crate::util::config_file::Config;
use crate::util::format_print::{format_resolve_chain, resolve_chain};
use crate::util::invocation::{self, ColorPolicy, Invocation, OutputFormat};
use crate::util::logger::{get_log_file_path, init_tracing, log_time, log_time_end};
use crate::util::presenter;
use crate::util::user_config::{
    init_registry, set_cache_dir, set_legacy_peer_deps, set_manifests_concurrency_limit,
    set_script_concurrency_limit,
};

mod cli;
mod cmd;
mod constants;
mod error;
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
        let category = classify(&e);
        // A failed package script propagates its own exit status so
        // `utoo run <script>` mirrors the script: a non-zero `exit N` becomes
        // N, and a signal death (e.g. SIGPIPE from `script | head`) becomes
        // 128+N. JSON invocations always use the stable category code.
        let exit_code = if invocation::json() {
            category.exit_code() as i32
        } else {
            e.downcast_ref::<ScriptExit>()
                .map_or_else(|| category.exit_code() as i32, |s| s.code)
        };
        if invocation::json() {
            let cli_error = e.downcast_ref::<CliError>();
            let message = cli_error.map_or_else(|| format!("{e:#}"), |e| e.message().to_string());
            let report = presenter::ErrorReport {
                command: invocation::command(),
                category,
                code: exit_code,
                message: &message,
                suggestion: cli_error.and_then(CliError::suggestion),
                required_by: resolve_chain(&e),
                details: cli_error.and_then(CliError::details),
            };
            let result = presenter::write_error(&mut io::stderr().lock(), &report);
            if result.is_err() {
                process::exit(exit_code);
            }
        } else if let Some(chain) = format_resolve_chain(&e) {
            eprintln!("error: {e:#}\n\n{chain}");
        } else {
            eprintln!("error: {e:#}");
        }
        if !invocation::json()
            && !invocation::quiet()
            && let Some(log_path) = get_log_file_path()
        {
            eprintln!("Full logs saved to: {}", log_path.display());
        }
        process::exit(exit_code);
    }
}

async fn async_main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let json_requested = args.iter().any(|arg| arg == "--json");
    let color = ColorPolicy::resolve(json_requested || args.iter().any(|arg| arg == "--no-color"));
    color.apply();

    // Preserve the custom root help catalog for human invocations. JSON help is
    // handled through clap below so stdout remains one machine document.
    if !json_requested && args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        let config = Config::load(ConfigScope::Local).await?;
        let config_service = ConfigService::new(config);
        config_service.print_help()?;
        return Ok(());
    }

    log_time(); // Start global timer
    let cli = match Cli::command()
        .color(color.clap_choice())
        .try_get_matches_from(&args)
        .and_then(|matches| Cli::from_arg_matches(&matches))
    {
        Ok(cli) => cli,
        Err(error) => {
            if json_requested {
                match error.kind() {
                    ClapErrorKind::DisplayHelp | ClapErrorKind::DisplayVersion => {
                        let command = if error.kind() == ClapErrorKind::DisplayVersion {
                            "version"
                        } else {
                            "help"
                        };
                        let output = if command == "version" {
                            serde_json::json!({ "version": APP_VERSION })
                        } else {
                            serde_json::json!({ "help": error.to_string().trim_end() })
                        };
                        presenter::write(&mut io::stdout().lock(), command, &output)?;
                        return Ok(());
                    }
                    _ => {
                        presenter::write_error(
                            &mut io::stderr().lock(),
                            &presenter::ErrorReport {
                                command: None,
                                category: ErrorKind::Usage,
                                code: 2,
                                message: error.to_string().trim(),
                                suggestion: None,
                                required_by: None,
                                details: None,
                            },
                        )?;
                        process::exit(2);
                    }
                }
            }
            error.exit();
        }
    };
    let color = ColorPolicy::resolve(cli.json || cli.no_color);
    color.apply();
    let execute_version = matches!(
        &cli.command,
        Some(Commands::Execute { command, .. }) if matches!(command.as_str(), "--version" | "-v")
    );
    let command = if cli.version || execute_version {
        Some("version")
    } else {
        cli.command
            .as_ref()
            .and_then(Commands::json_name)
            .or_else(|| (cli.command.is_none() && cli.script_name.is_none()).then_some("install"))
    };
    invocation::init(Invocation {
        output: OutputFormat::from(cli.json),
        quiet: cli.quiet,
        color,
        command,
    });

    // Handle version before the unsupported-command gate: `--json --version`
    // is itself a machine-readable command result.
    if cli.version || execute_version {
        let output = serde_json::json!({ "version": APP_VERSION });
        return presenter::emit("version", &output, || {
            println!("{APP_VERSION}");
            Ok(())
        });
    }

    if cli.json {
        let unsupported_command = cli
            .command
            .as_ref()
            .is_some_and(|command| !command.supports_json());
        if cli.script_name.is_some() || unsupported_command {
            return Err(CliError::usage("--json is not supported by this command yet").into());
        }
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
    let (log_file, _guard) = init_tracing(cli.verbose, cli.quiet || cli.json, color)
        .context("Failed to initialize logging")?;

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

    // Concurrency limits — the setters ignore `None`, so passing the CLI option
    // straight through applies an override only when one was given.
    set_manifests_concurrency_limit(cli.manifests_concurrency_limit);
    set_script_concurrency_limit(cli.script_concurrency_limit);

    // Auto update: check cache → update or refresh in background
    init_auto_update().await;

    match cli.command {
        Some(Commands::Clean { pattern, yes }) => {
            clean(&pattern, yes).await?;
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
        Some(Commands::Publish {
            tag,
            dry_run,
            otp,
            access,
            // utoo performs no Git cleanliness checks, so `--no-git-checks` is a
            // documented no-op accepted for pnpm/npm compatibility.
            no_git_checks: _,
            provenance,
        }) => {
            // `--workspace`/`--filter` selects member(s); empty means the current
            // package. `--workspaces` is intentionally NOT honored here to avoid
            // an accidental publish of every member.
            let filter = if cli.workspace.is_empty() {
                WorkspaceFilter::Current
            } else {
                WorkspaceFilter::Selected(cli.workspace)
            };
            cmd::publish::publish(
                tag.as_deref(),
                dry_run.into(),
                otp.as_deref(),
                access,
                filter,
                provenance,
            )
            .await?;
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
