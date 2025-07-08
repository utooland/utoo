use std::process;

use anyhow::Result;
use clap::{Parser, Subcommand};
use cmd::deps::build_deps;
use cmd::execute::execute;
use cmd::install::{install, install_global_package, update_packages};
use cmd::rebuild::rebuild;
use cmd::run::run;
use cmd::update::update;
use cmd::{clean::clean, deps::build_workspace};
use helper::auto_update::init_auto_update;
use util::config::{set_legacy_peer_deps, set_registry};
use util::logger::{log_error, log_info, log_warning, set_verbose, write_verbose_logs_to_file};
use util::save_type::{PackageAction, SaveType, parse_save_type};

mod cmd;
mod constants;
mod helper;
mod model;
mod service;
mod util;

use crate::constants::cmd::{
    CLEAN_ABOUT, CLEAN_NAME, DEPS_ABOUT, DEPS_NAME, EXECUTE_ABOUT, EXECUTE_NAME, INSTALL_ABOUT,
    INSTALL_NAME, REBUILD_ABOUT, REBUILD_NAME, UNINSTALL_ABOUT, UNINSTALL_NAME, UPDATE_ABOUT,
};
use crate::constants::{APP_ABOUT, APP_NAME, APP_VERSION};
use crate::helper::cli::parse_script_and_args;
use crate::helper::workspace::update_cwd_to_root;

#[derive(Parser)]
#[command(name = APP_NAME)]
#[command(version = APP_VERSION)]
#[command(about = APP_ABOUT)]
#[command(allow_external_subcommands(true))]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long)]
    ignore_scripts: bool,

    #[arg(long, global = true)]
    verbose: bool,

    #[arg(long, global = true)]
    registry: Option<String>,

    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    legacy_peer_deps: Option<bool>,

    #[arg(short = 'v', long)]
    version: bool,

    /// Workspace to operate in
    #[arg(short, long, global = true, hide = true)]
    workspace: Option<String>,

    /// Workspace to operate in
    #[arg(short, long, global = true, hide = true, default_value = "false")]
    workspaces: bool,

    script_name: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Install dependencies
    #[command(name = INSTALL_NAME, alias = "i", about = INSTALL_ABOUT)]
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
    #[command(name = UNINSTALL_NAME, alias = "un", about = UNINSTALL_ABOUT)]
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

    #[command(name = REBUILD_NAME, alias = "rb", about = REBUILD_ABOUT)]
    Rebuild,

    #[command(name = CLEAN_NAME, alias = "c", about = CLEAN_ABOUT)]
    Clean {
        #[arg(default_value = "*")]
        pattern: String,
    },

    #[command(name = DEPS_NAME, alias = "d", about = DEPS_ABOUT)]
    Deps {
        #[arg(long)]
        workspace_only: bool,
    },

    #[command(name = "update", alias = "u", about = UPDATE_ABOUT)]
    Update,

    /// Run scripts defined in package.json
    #[command(name = "run", alias = "r")]
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
    #[command(name = EXECUTE_NAME, alias = "x", about = EXECUTE_ABOUT)]
    Execute {
        /// Command to execute
        command: String,

        /// Arguments to pass to the command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        libc::umask(0o00);
    }

    let cli = Cli::parse();

    // global verbose
    set_verbose(cli.verbose);

    // global registry
    set_registry(cli.registry);

    // set legacy_peer_deps when set --legacy
    if cli.legacy_peer_deps == Some(true) {
        set_legacy_peer_deps(cli.legacy_peer_deps);
    }

    // Handle --version flag
    if cli.version {
        println!("{APP_VERSION}");
        return Ok(());
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
                    for spec in specs {
                        if let Err(e) = install_global_package(&spec, &prefix).await {
                            log_error(&e.to_string());
                            let _ = write_verbose_logs_to_file();
                            process::exit(1);
                        }
                    }
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
                }
            } else {
                let cwd = std::env::current_dir()?;
                let root_path = update_cwd_to_root(&cwd).await?;
                if let Err(e) = install(ignore_scripts, &root_path).await {
                    log_error(&e.to_string());
                    let _ = write_verbose_logs_to_file();
                    process::exit(1);
                }
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
            } else {
                return Err("Package specification is required for uninstall".into());
            }
        }
        Some(Commands::Rebuild) => {
            log_info("Executing dependency hook scripts and creating node_modules/.bin links");
            let cwd = std::env::current_dir()?;
            if let Err(e) = rebuild(&cwd).await {
                log_error(&e.to_string());
                let _ = write_verbose_logs_to_file();
                process::exit(1);
            }
            log_info("💫 All dependencies rebuild completed");
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
            }
        }
        Some(Commands::Update) => {
            if let Err(e) = update(false).await {
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
        None => {
            // Check if the first argument is a script name
            if let Some(script_name) = std::env::args().nth(1) {
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
            }
        }
    }

    Ok(())
}
