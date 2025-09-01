#![feature(let_chains)]

use std::process;

use anyhow::Result;
use clap::{Parser, Subcommand};
use cmd::config::{handle_config_get, handle_config_list, handle_config_set};
use cmd::deps::build_deps;
use cmd::execute::execute;
use cmd::install::{install, install_global_package, update_packages};
use cmd::link::{link_current_to_global, link_global_to_local};
use cmd::list::list_dependencies;
use cmd::rebuild::rebuild;
use cmd::run::run;
use cmd::update::update;
use cmd::view::view;
use cmd::{clean::clean, deps::build_workspace};
use helper::auto_update::init_auto_update;
use util::config::{set_legacy_peer_deps, set_registry};
use util::logger::{
    log_error, log_time, log_time_end, log_warning, set_verbose, write_verbose_logs_to_file,
};
use util::save_type::{PackageAction, SaveType, parse_save_type};

mod cmd;
mod constants;
mod helper;
mod model;
mod service;
mod util;

use crate::constants::cmd::{
    CLEAN_ABOUT, CLEAN_ALIAS, CLEAN_NAME, CONFIG_ABOUT, CONFIG_ALIAS, CONFIG_NAME, DEPS_ABOUT,
    DEPS_ALIAS, DEPS_NAME, EXECUTE_ABOUT, EXECUTE_ALIAS, EXECUTE_NAME, INSTALL_ABOUT,
    INSTALL_ALIAS, INSTALL_NAME, LINK_ABOUT, LINK_ALIAS, LINK_NAME, LIST_ALIAS, LIST_NAME,
    REBUILD_ABOUT, REBUILD_ALIAS, REBUILD_NAME, RUN_ALIAS, RUN_NAME, UNINSTALL_ABOUT,
    UNINSTALL_ALIAS, UNINSTALL_NAME, UPDATE_ABOUT, UPDATE_ALIAS, UPDATE_NAME, VIEW_ABOUT,
    VIEW_ALIAS, VIEW_NAME,
};
use crate::constants::{APP_ABOUT, APP_NAME, APP_VERSION};
use crate::helper::cli::parse_script_and_args;
use crate::helper::workspace::update_cwd_to_root;

#[derive(Parser)]
#[command(name = APP_NAME)]
#[command(version = APP_VERSION)]
#[command(about = APP_ABOUT)]
#[command(allow_external_subcommands(true))]
#[command(disable_version_flag(true))]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long)]
    ignore_scripts: bool,

    #[arg(short = 'v', long = "version")]
    version: bool,

    #[arg(long, global = true)]
    verbose: bool,

    #[arg(long, global = true)]
    registry: Option<String>,

    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    legacy_peer_deps: Option<bool>,

    /// Workspace to operate in
    #[arg(long, global = true, hide = true)]
    workspace: Option<String>,

    /// Workspace to operate in
    #[arg(long, global = true, hide = true, default_value = "false")]
    workspaces: bool,

    script_name: Option<String>,
}

#[derive(Subcommand)]
enum ConfigCommands {
    #[command(about = "Set a configuration value with the specified key")]
    Set {
        key: String,
        value: String,
        #[arg(long)]
        global: bool,
    },
    #[command(about = "Retrieve a configuration value by its key")]
    Get {
        key: String,
        #[arg(long)]
        global: bool,
        #[arg(allow_hyphen_values = true)]
        #[arg(trailing_var_arg = true)]
        override_values: Vec<String>,
    },
    #[command(about = "Display all configuration key-value pairs")]
    List {
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Install dependencies
    #[command(name = INSTALL_NAME, alias = INSTALL_ALIAS, about = INSTALL_ABOUT)]
    Install {
        /// Package specifications (e.g. "lodash@4.17.21" "react@18.0.0")
        specs: Vec<String>,

        /// Workspace to install in
        #[arg(short, long)]
        workspace: Option<String>,

        /// Skip running dependency scripts
        #[arg(long)]
        ignore_scripts: bool,

        /// Save as dev dependency
        #[arg(long, short = 'D')]
        save_dev: bool,

        /// Save as peer dependency
        #[arg(long)]
        save_peer: bool,

        /// Save as optional dependency
        #[arg(long, short = 'O')]
        save_optional: bool,

        /// Install package globally
        #[arg(short, long)]
        global: bool,

        #[arg(short, long)]
        prefix: Option<String>,
    },
    /// Uninstall dependencies
    #[command(name = UNINSTALL_NAME, alias = UNINSTALL_ALIAS, about = UNINSTALL_ABOUT)]
    Uninstall {
        /// Package specifications (e.g. "lodash@4.17.21" "react@18.0.0")
        specs: Vec<String>,

        /// Workspace to uninstall from
        #[arg(short, long)]
        workspace: Option<String>,

        /// Skip running dependency scripts
        #[arg(long)]
        ignore_scripts: bool,
    },

    #[command(name = REBUILD_NAME, alias = REBUILD_ALIAS, about = REBUILD_ABOUT)]
    Rebuild,

    #[command(name = CLEAN_NAME, alias = CLEAN_ALIAS, about = CLEAN_ABOUT)]
    Clean {
        #[arg(default_value = "*")]
        pattern: String,
    },

    #[command(name = DEPS_NAME, alias = DEPS_ALIAS, about = DEPS_ABOUT)]
    Deps {
        #[arg(long)]
        workspace_only: bool,
    },

    #[command(name = UPDATE_NAME, alias = UPDATE_ALIAS, about = UPDATE_ABOUT)]
    Update,

    /// List dependencies like npm list
    #[command(name = LIST_NAME, alias = LIST_ALIAS)]
    List {
        /// Package name to show dependencies for
        #[arg(value_name = "PACKAGE")]
        package: String,
    },

    /// Run scripts defined in package.json
    #[command(name = RUN_NAME, alias = RUN_ALIAS)]
    Run {
        /// Script name to run
        script: String,

        /// Workspace to run script in
        #[arg(short, long)]
        workspace: Option<String>,

        /// Run script in all workspaces with topological ordering
        #[arg(long)]
        workspaces: bool,
    },

    /// Execute packages similar to npx
    #[command(name = EXECUTE_NAME, alias = EXECUTE_ALIAS, about = EXECUTE_ABOUT)]
    Execute {
        /// Command to execute
        command: String,

        /// Arguments to pass to the command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    #[command(name = VIEW_NAME, alias = VIEW_ALIAS, about = VIEW_ABOUT)]
    View {
        /// Package name to view
        package: String,
    },

    /// Link current package to global or create symlink to global package
    #[command(name = LINK_NAME, alias = LINK_ALIAS, about = LINK_ABOUT)]
    Link {
        /// Package name to link from global (if not provided, links current package to global)
        packages: Option<Vec<String>>,

        /// prefix for global package path
        #[arg(short, long)]
        prefix: Option<String>,
    },

    #[command(name = CONFIG_NAME, alias = CONFIG_ALIAS, about = CONFIG_ABOUT)]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Check for help flag
    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        let config = crate::util::config::Config::load(false)?;
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

    // global verbose
    set_verbose(cli.verbose);

    // global registry
    set_registry(cli.registry);

    // set legacy_peer_deps when set --legacy
    if cli.legacy_peer_deps == Some(true) {
        set_legacy_peer_deps(cli.legacy_peer_deps);
    }

    // Ensure the version is up to date, weak dependency
    if let Err(_e) = init_auto_update().await {
        log_warning("Auto update cancelled");
    }

    match cli.command {
        Some(Commands::Clean { pattern }) => {
            if let Err(e) = clean(&pattern).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            } else {
                log_time_end(&format!("{pattern} cleaned"));
            }
        }
        Some(Commands::Install {
            specs,
            workspace,
            ignore_scripts,
            save_dev,
            save_peer,
            save_optional,
            global,
            prefix,
        }) => {
            if !specs.is_empty() {
                if global {
                    // For global installs, process packages one by one
                    for spec in specs.iter() {
                        if let Err(e) = install_global_package(spec, &prefix.as_deref()).await {
                            log_error(&e.to_string());
                            let _ = write_verbose_logs_to_file();
                            process::exit(1);
                        }
                    }
                    log_time_end(&format!(
                        "{} package{} installed",
                        specs.len(),
                        if specs.len() == 1 { "" } else { "s" }
                    ));
                } else {
                    let save_type = parse_save_type(save_dev, save_peer, save_optional);
                    let spec_refs: Vec<&str> = specs.iter().map(|s| s.as_str()).collect();
                    if let Err(e) = update_packages(
                        PackageAction::Add,
                        &spec_refs,
                        workspace.clone(),
                        ignore_scripts,
                        save_type,
                    )
                    .await
                    {
                        log_error(&e.to_string());
                        let _ = write_verbose_logs_to_file();
                        process::exit(1);
                    }
                    // Log install result with correct singular/plural form in one line
                    log_time_end(&format!(
                        "{} package{} installed",
                        specs.len(),
                        if specs.len() == 1 { "" } else { "s" }
                    ));
                }
            } else {
                let cwd = std::env::current_dir()?;
                let root_path = update_cwd_to_root(&cwd).await?;
                if let Err(e) = install(ignore_scripts, &root_path).await {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
                log_time_end("All packages installed");
            }
        }
        Some(Commands::Uninstall {
            specs,
            workspace,
            ignore_scripts,
        }) => {
            if !specs.is_empty() {
                let spec_refs: Vec<&str> = specs.iter().map(|s| s.as_str()).collect();
                if let Err(e) = update_packages(
                    PackageAction::Remove,
                    &spec_refs,
                    workspace.clone(),
                    ignore_scripts,
                    SaveType::Prod,
                )
                .await
                {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
                log_time_end(&format!(
                    "{} package{} uninstalled",
                    specs.len(),
                    if specs.len() == 1 { "" } else { "s" }
                ));
            } else {
                return Err("Package specification is required for uninstall".into());
            }
        }
        Some(Commands::Rebuild) => {
            let cwd = std::env::current_dir()?;
            if let Err(e) = rebuild(&cwd).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
            log_time_end("All packages rebuilded");
        }
        Some(Commands::Deps { workspace_only }) => {
            let cwd = std::env::current_dir()?;
            let root_path = update_cwd_to_root(&cwd).await?;
            let result = if workspace_only {
                build_workspace(&root_path).await
            } else {
                build_deps(&root_path).await
            };

            if let Err(e) = result {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            } else {
                log_time_end("deps resolved");
            }
        }
        Some(Commands::Update) => {
            if let Err(e) = update(false).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
            log_time_end("All packages updated");
        }
        Some(Commands::List { package }) => {
            let cwd = std::env::current_dir()?;

            if let Err(e) = list_dependencies(&cwd, &package).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
        }
        Some(Commands::Execute { command, args }) => {
            if let Err(e) = execute(&command, args).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
        }
        Some(Commands::Run {
            script,
            workspace,
            workspaces,
        }) => {
            let args = std::env::args().skip(2).collect::<Vec<String>>();
            let script_args = parse_script_and_args(&args);
            let script_args_owned = script_args.map(|args| {
                args.into_iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            });

            if let Err(e) = run(&script, workspace.as_deref(), workspaces, script_args_owned).await
            {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
        }
        Some(Commands::View { package }) => {
            if let Err(e) = view(&package).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
        }
        Some(Commands::Link { packages, prefix }) => {
            match packages {
                None => {
                    // Link current package to global
                    if let Err(e) = link_current_to_global(prefix.as_deref()).await {
                        log_error(&e.to_string());
                        let _ = write_verbose_logs_to_file();
                        process::exit(1);
                    }
                    log_time_end("package linked");
                }
                Some(packages) => {
                    for package in packages.iter() {
                        if let Err(e) = link_global_to_local(&package, prefix.as_deref()).await {
                            log_error(&e.to_string());
                            let _ = write_verbose_logs_to_file();
                            process::exit(1);
                        }
                    }
                    log_time_end(&format!("'{}' linked to local", packages.join(", ")));
                }
            }
        }
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Set { key, value, global } => {
                if let Err(e) = handle_config_set(key, value, global) {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
            }
            ConfigCommands::Get {
                key,
                global,
                override_values,
            } => {
                if let Err(e) = handle_config_get(key, global, override_values) {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
            }
            ConfigCommands::List { global } => {
                if let Err(e) = handle_config_list(global) {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
            }
        },
        None => {
            // Check if the first argument is a script name
            if let Some(script_name) = std::env::args().nth(1) {
                // First check if there's a custom command configured for this script name
                let config = crate::util::config::Config::load(false)?;
                let config_service = crate::service::config::ConfigService::new(config);
                // Check if there's a custom command available
                if let Ok(Some(_)) = config_service.get_available_cmd(&script_name) {
                    // Execute the custom command
                    config_service.execute_command(
                        &script_name,
                        &std::env::args().skip(2).collect::<Vec<String>>(),
                    )?;
                    return Ok(());
                }

                // If no custom command found, try to run as script
                let args = std::env::args().skip(1).collect::<Vec<String>>();
                let script_args = parse_script_and_args(&args);
                let script_args_owned = script_args.map(|args| {
                    args.into_iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>()
                });

                if let Err(e) = run(
                    &script_name,
                    cli.workspace.as_deref(),
                    cli.workspaces,
                    script_args_owned,
                )
                .await
                {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
            } else {
                // Default to install if no arguments
                let cwd = std::env::current_dir()?;
                let root_path = update_cwd_to_root(&cwd).await?;
                if let Err(e) = install(cli.ignore_scripts, &root_path).await {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
                log_time_end("All packages installed");
            }
        }
    }

    Ok(())
}
