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
use crate::model::cli_output::{
    CompletionsResult, ErrorDetails, HelpResult, HelpTarget, InitResult, RequestedPackage,
    RequiredBy, VersionResult,
};
use crate::service::config::ConfigService;
use crate::service::script::{MissingScript, ScriptExit, script_failure_details};
use crate::service::workspace::WorkspaceFilter;
use crate::util::cli_enum::{
    ColorPolicy, ConfigScope, ConfirmationPolicy, ConsoleVerbosity, InitMode, OutputFormat,
    ProvenancePolicy, ScriptPolicy,
};
use crate::util::config_file::Config;
use crate::util::format_print::{format_resolve_chain, resolve_chain};
use crate::util::invocation::{self, Invocation};
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
    invocation::start();
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
            let message = cli_error.map_or_else(|| e.to_string(), |e| e.message().to_string());
            let causes = e
                .chain()
                .skip(1)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let script_details = script_failure_details(&e);
            let dependency_details = dependency_failure_details(&e);
            let code = cli_error.map_or_else(
                || {
                    if script_details.is_some() {
                        "script_failed"
                    } else if dependency_details.is_some() {
                        "dependency_resolution_failed"
                    } else {
                        "operation_failed"
                    }
                },
                CliError::code,
            );
            let report = presenter::ErrorReport {
                command: invocation::command(),
                subcommand: invocation::subcommand(),
                category,
                code,
                exit_code: category.exit_code(),
                message: &message,
                causes: &causes,
                suggestion: cli_error.and_then(CliError::suggestion),
                partial_result: cli_error.and_then(CliError::partial_result),
                details: cli_error
                    .and_then(CliError::details)
                    .or(script_details.as_ref())
                    .or(dependency_details.as_ref()),
                log_path: get_log_file_path().map(|path| path.to_string_lossy().into_owned()),
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
        if !invocation::quiet()
            && let Some(log_path) = get_log_file_path()
        {
            eprintln!("Full logs saved to: {}", log_path.display());
        }
        process::exit(exit_code);
    }
}

async fn async_main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let json_requested = has_flag_before_delimiter(&args, "--json");
    let no_color_requested = has_flag_before_delimiter(&args, "--no-color");
    let color = ColorPolicy::from(
        json_requested || no_color_requested || std::env::var_os("NO_COLOR").is_some(),
    );
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
                        if command == "version" {
                            presenter::write(
                                &mut io::stdout().lock(),
                                command,
                                None,
                                &VersionResult {
                                    version: APP_VERSION.to_string(),
                                },
                            )?;
                        } else {
                            presenter::write(
                                &mut io::stdout().lock(),
                                command,
                                None,
                                &HelpResult {
                                    target: detect_help_target(&args),
                                    text: error.to_string().trim_end().to_string(),
                                },
                            )?;
                        }
                        return Ok(());
                    }
                    _ => {
                        let causes = Vec::new();
                        presenter::write_error(
                            &mut io::stderr().lock(),
                            &presenter::ErrorReport {
                                command: detect_command(&args),
                                subcommand: detect_subcommand(&args),
                                category: ErrorKind::Usage,
                                code: "invalid_arguments",
                                exit_code: ErrorKind::Usage.exit_code(),
                                message: error.to_string().trim(),
                                causes: &causes,
                                suggestion: None,
                                partial_result: None,
                                details: None,
                                log_path: None,
                            },
                        )?;
                        process::exit(2);
                    }
                }
            }
            error.exit();
        }
    };
    let color =
        ColorPolicy::from(cli.json || cli.no_color || std::env::var_os("NO_COLOR").is_some());
    color.apply();
    let verbosity = ConsoleVerbosity::from_flags(cli.verbose, cli.quiet || cli.json);
    let execute_version = matches!(
        &cli.command,
        Some(Commands::Execute { command, .. }) if matches!(command.as_str(), "--version" | "-v")
    );
    let command = if cli.version || execute_version {
        Some("version")
    } else {
        cli.command
            .as_ref()
            .map(Commands::json_name)
            .or_else(|| cli.script_name.as_ref().map(|_| "run"))
            .or_else(|| (cli.command.is_none() && cli.script_name.is_none()).then_some("install"))
    };
    let subcommand = cli.command.as_ref().and_then(Commands::json_subcommand);
    invocation::init(Invocation {
        output: OutputFormat::from(cli.json),
        verbosity,
        color,
        command,
        subcommand,
    });

    // Handle version before the unsupported-command gate: `--json --version`
    // is itself a machine-readable command result.
    if cli.version || execute_version {
        let output = VersionResult {
            version: APP_VERSION.to_string(),
        };
        return presenter::emit("version", &output, || {
            println!("{APP_VERSION}");
            Ok(())
        });
    }

    // Handle completions early to avoid unnecessary initialization (tracing, registry, auto-update)
    if let Some(Commands::Completions { shell }) = cli.command {
        let shell = shell.or_else(detect_shell_from_env);

        let Some(shell) = shell else {
            if invocation::json() {
                return Err(CliError::usage(
                    "could not detect shell; specify bash, zsh, fish, powershell, or elvish",
                )
                .into());
            }
            eprintln!(
                "Could not detect shell. Usage: utoo completions <bash|zsh|fish|powershell|elvish>"
            );
            process::exit(2);
        };

        if invocation::json() {
            let script = tokio::task::spawn_blocking(move || {
                let mut output = Vec::new();
                let mut cmd = Cli::command();
                clap_complete::generate(shell, &mut cmd, APP_NAME, &mut output);
                String::from_utf8(output).context("Generated completion script is not UTF-8")
            })
            .await
            .context("Failed to generate shell completions")??;
            return presenter::emit(
                "completions",
                &CompletionsResult {
                    shell: shell.to_string(),
                    script,
                },
                || Ok(()),
            );
        }

        tokio::task::spawn_blocking(move || {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, APP_NAME, &mut std::io::stdout());
        })
        .await
        .context("Failed to generate shell completions")?;

        return Ok(());
    }

    if cli.json && matches!(&cli.command, Some(Commands::Login)) {
        return Err(
            CliError::usage("login requires an interactive browser flow")
                .with_code("interactive_required")
                .with_suggestion("run without `--json`")
                .into(),
        );
    }

    // Initialize tracing (replaces set_verbose)
    let (log_file, _guard) =
        init_tracing(verbosity, color).context("Failed to initialize logging")?;

    tracing::debug!(
        log_file = %log_file.display(),
        ?verbosity,
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
            let confirmation = ConfirmationPolicy::from(yes);
            if confirmation.requires_interaction() && !invocation::interactive() {
                return Err(CliError::usage(
                    "refusing to prompt for cache deletion in non-interactive mode",
                )
                .with_suggestion("rerun with `utoo clean --yes`")
                .into());
            }
            clean(&pattern, confirmation).await?;
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
        Some(Commands::Update(args)) => {
            update(args, ScriptPolicy::Run).await?;
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
            let mode = InitMode::from(yes);
            if mode.requires_interaction() && !invocation::interactive() {
                return Err(CliError::usage(
                    "refusing to prompt for package metadata in non-interactive mode",
                )
                .with_suggestion("re-run with `utoo init --yes`")
                .into());
            }
            let output = if invocation::json() {
                service::init::InitOutput::Machine
            } else {
                service::init::InitOutput::Human
            };
            service::init::init(mode, output, None).await?;
            log_time_end("package.json created");
            if invocation::json() {
                let path = std::env::current_dir()?.join("package.json");
                let package: serde_json::Value =
                    serde_json::from_str(&crate::fs::read_to_string(&path).await?)?;
                presenter::emit(
                    "init",
                    &InitResult {
                        path: path.to_string_lossy().into_owned(),
                        name: package["name"].as_str().unwrap_or_default().to_string(),
                        version: package["version"].as_str().unwrap_or_default().to_string(),
                    },
                    || Ok(()),
                )?;
            }
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
                ProvenancePolicy::from(provenance),
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

fn has_flag_before_delimiter(args: &[String], flag: &str) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == flag)
}

fn detect_help_target(args: &[String]) -> Option<HelpTarget> {
    let command = detect_command(args)?;
    (command != "help" && command != "version").then(|| HelpTarget {
        command: command.to_string(),
        subcommand: detect_subcommand(args).map(str::to_string),
    })
}

fn detect_command(args: &[String]) -> Option<&'static str> {
    detect_command_token(args).map(|(command, _)| command)
}

fn detect_command_token(args: &[String]) -> Option<(&'static str, usize)> {
    let mut index = 1;
    while index < args.len() {
        let value = args[index].as_str();
        if value == "--" {
            return None;
        }
        if matches!(
            value,
            "--registry"
                | "--cache-dir"
                | "--manifests-concurrency-limit"
                | "--script-concurrency-limit"
                | "--workspace"
                | "--filter"
        ) {
            index += 2;
            continue;
        }
        if value.starts_with('-') {
            index += 1;
            continue;
        }
        let command = match value {
            "i" | "add" | "install" => "install",
            "un" | "uninstall" => "uninstall",
            "rb" | "rebuild" => "rebuild",
            "c" | "clean" => "clean",
            "d" | "deps" => "deps",
            "u" | "update" => "update",
            "ls" | "list" => "list",
            "r" | "run" => "run",
            "x" | "execute" => "execute",
            "v" | "view" | "info" | "show" => "view",
            "ln" | "link" => "link",
            "pk" | "pm-pack" => "pack",
            "pub" | "publish" => "publish",
            "pg" | "ping" => "ping",
            "lg" | "login" => "login",
            "who" | "whoami" => "whoami",
            "lo" | "logout" => "logout",
            "cfg" | "config" => "config",
            "create" | "init" => "init",
            "cmp" | "completions" => "completions",
            _ => "run",
        };
        return Some((command, index));
    }
    None
}

fn detect_subcommand(args: &[String]) -> Option<&'static str> {
    let (command, index) = detect_command_token(args)?;
    if command != "config" {
        return None;
    }
    let mut index = index + 1;
    while index < args.len() {
        let value = args[index].as_str();
        if value == "--" {
            return None;
        }
        if matches!(
            value,
            "--registry"
                | "--cache-dir"
                | "--manifests-concurrency-limit"
                | "--script-concurrency-limit"
        ) {
            index += 2;
            continue;
        }
        if value.starts_with('-') {
            index += 1;
            continue;
        }
        return match value {
            "set" => Some("set"),
            "get" => Some("get"),
            "list" => Some("list"),
            _ => None,
        };
    }
    None
}

fn dependency_failure_details(error: &anyhow::Error) -> Option<ErrorDetails> {
    let chain = resolve_chain(error)?;
    let (requested, required_by) = chain.split_last()?;
    Some(ErrorDetails::Dependency {
        package: RequestedPackage {
            name: requested.0.clone(),
            spec: requested.1.clone(),
        },
        required_by: required_by
            .iter()
            .map(|(name, version)| RequiredBy {
                name: (!name.is_empty()).then(|| name.clone()),
                version: (!version.is_empty()).then(|| version.clone()),
            })
            .collect(),
    })
}
