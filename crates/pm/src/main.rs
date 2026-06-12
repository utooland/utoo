use std::process;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use cmd::config::{handle_config_get, handle_config_list, handle_config_set};
use cmd::deps::build_deps;
use cmd::install::{install, install_global_package, update_packages};
use cmd::link::{link_current_to_global, link_global_to_local};
use cmd::list::list_dependencies;
use cmd::rebuild::rebuild;
use cmd::run::run;
use cmd::update::update;
use cmd::view::view;
use cmd::{clean::clean, deps::build_workspace};
use helper::auto_update::init_auto_update;
use service::script::MissingScript;
use service::workspace::WorkspaceFilter;
use util::cli_enum::{
    ConfigScope, OmitType, PackageAction, SaveType, ScriptPolicy, parse_save_type,
};
use util::format_print::pluralized_package_count;
use util::logger::{get_log_file_path, init_tracing, log_time, log_time_end};
use util::user_config::{
    InstallScope, init_registry, set_cache_dir, set_install_scope, set_legacy_peer_deps,
    set_manifests_concurrency_limit, set_omit,
};

mod cmd;
mod constants;
mod fs;
mod helper;
mod model;
mod service;
mod util;

use crate::constants::cmd::{
    CLEAN_ABOUT, CLEAN_ALIAS, CLEAN_NAME, COMPLETIONS_ABOUT, COMPLETIONS_ALIAS, COMPLETIONS_NAME,
    CONFIG_ABOUT, CONFIG_ALIAS, CONFIG_NAME, DEPS_ABOUT, DEPS_ALIAS, DEPS_NAME, EXECUTE_ABOUT,
    EXECUTE_ALIAS, EXECUTE_NAME, INIT_ABOUT, INIT_ALIAS, INIT_NAME, INSTALL_ABOUT, INSTALL_NAME,
    LINK_ABOUT, LINK_ALIAS, LINK_NAME, LIST_ALIAS, LIST_NAME, LOGIN_ABOUT, LOGIN_ALIAS, LOGIN_NAME,
    LOGOUT_ABOUT, LOGOUT_ALIAS, LOGOUT_NAME, PACK_ABOUT, PACK_ALIAS, PACK_NAME, PING_ABOUT,
    PING_ALIAS, PING_NAME, PUBLISH_ABOUT, PUBLISH_ALIAS, PUBLISH_NAME, REBUILD_ABOUT,
    REBUILD_ALIAS, REBUILD_NAME, RUN_ALIAS, RUN_NAME, UNINSTALL_ABOUT, UNINSTALL_ALIAS,
    UNINSTALL_NAME, UPDATE_ABOUT, UPDATE_ALIAS, UPDATE_NAME, VIEW_ABOUT, VIEW_ALIAS,
    VIEW_ALIAS_INFO, VIEW_ALIAS_SHOW, VIEW_NAME, WHOAMI_ABOUT, WHOAMI_ALIAS, WHOAMI_NAME,
};
use crate::constants::{APP_ABOUT, APP_NAME, APP_VERSION};
use crate::helper::workspace::init_project_root;

use crate::helper::migrate::{FromPm, migrate_from_pnpm};

fn detect_shell_from_env() -> Option<clap_complete::Shell> {
    // Most common on Unix-like systems.
    let shell_path = std::env::var("SHELL").ok()?;
    let name = shell_path.rsplit('/').next().unwrap_or(shell_path.as_str());

    match name {
        "bash" => Some(clap_complete::Shell::Bash),
        "zsh" => Some(clap_complete::Shell::Zsh),
        "fish" => Some(clap_complete::Shell::Fish),
        // Leave PowerShell + Elvish to explicit flags; auto-detect tends to be unreliable.
        _ => None,
    }
}

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

    #[arg(long, global = true)]
    cache_dir: Option<String>,

    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    legacy_peer_deps: Option<bool>,

    /// Maximum concurrent manifest fetches (default: 64)
    #[arg(long, global = true)]
    manifests_concurrency_limit: Option<usize>,

    /// Workspace to operate in (may be repeated; supports glob patterns)
    #[arg(long, global = true, hide = true, num_args = 1)]
    workspace: Vec<String>,

    /// Run in all workspaces with topological ordering
    #[arg(long, global = true, hide = true, default_value = "false")]
    workspaces: bool,

    script_name: Option<String>,

    /// Arguments to pass to the script when running without explicit subcommand
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    script_args: Vec<String>,
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
    #[command(name = INSTALL_NAME, aliases = ["i", "add"], about = INSTALL_ABOUT)]
    Install {
        /// Package specifications (e.g. "lodash@4.17.21" "react@18.0.0")
        specs: Vec<String>,

        /// Workspace to install in
        #[arg(short, long)]
        workspace: Option<String>,

        /// Skip running dependency scripts
        #[arg(long)]
        ignore_scripts: bool,

        /// Save as production dependency (default behavior)
        #[arg(long, short = 'S', default_value_t = true)]
        save: bool,

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

        /// Only install production dependencies (omit dev and optional)
        #[arg(long)]
        production: bool,

        /// Dependency types to omit
        #[arg(long, value_delimiter = ',')]
        omit: Vec<OmitType>,

        /// Migrate from another package manager before installing
        #[arg(long)]
        from: Option<FromPm>,
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
        /// Script name to run (optional, will prompt if not provided)
        script: Option<String>,

        /// Workspace(s) to run script in. Repeatable; supports glob patterns
        /// (e.g. `--workspace packages/a --workspace 'packages/*'`).
        #[arg(short, long, num_args = 1)]
        workspace: Vec<String>,

        /// Run script in all workspaces with topological ordering
        #[arg(long)]
        workspaces: bool,

        /// Skip workspaces that don't have the specified script
        #[arg(long)]
        if_present: bool,

        /// Arguments to pass to the script
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Execute packages similar to npx
    #[command(name = EXECUTE_NAME, alias = EXECUTE_ALIAS, about = EXECUTE_ABOUT)]
    Execute {
        /// Command (package) to execute, or `--version` for `x` itself.
        ///
        /// `allow_hyphen_values` lets a leading `--version`/`-v` reach here
        /// (handled in `main`) instead of clap rejecting it; a real package name
        /// never starts with `-`, and known global flags (e.g. `--registry`) are
        /// still parsed before this positional. (`--help`/`-h` is handled by
        /// clap's built-in help for the subcommand.)
        #[arg(allow_hyphen_values = true)]
        command: String,

        /// Arguments to pass to the command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    #[command(name = VIEW_NAME, alias = VIEW_ALIAS, about = VIEW_ABOUT)]
    #[command(aliases = [VIEW_ALIAS_INFO, VIEW_ALIAS_SHOW])]
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

    #[command(name = PACK_NAME, alias = PACK_ALIAS, about = PACK_ABOUT)]
    Pack {
        /// Path to the package directory
        path: Option<String>,
        /// Perform a dry run without creating a tarball
        #[arg(long)]
        dry_run: bool,
    },

    #[command(name = PUBLISH_NAME, alias = PUBLISH_ALIAS, about = PUBLISH_ABOUT)]
    Publish {
        /// Distribution tag (default: latest, or publishConfig.tag from package.json)
        #[arg(long)]
        tag: Option<String>,
        /// Perform a dry run without publishing
        #[arg(long)]
        dry_run: bool,
        /// One-time password for 2FA
        #[arg(long)]
        otp: Option<String>,
    },

    #[command(name = PING_NAME, alias = PING_ALIAS, about = PING_ABOUT)]
    Ping {
        /// Registry URL to ping (defaults to configured registry)
        registry: Option<String>,
    },

    #[command(name = LOGIN_NAME, alias = LOGIN_ALIAS, about = LOGIN_ABOUT)]
    Login,

    #[command(name = WHOAMI_NAME, alias = WHOAMI_ALIAS, about = WHOAMI_ABOUT)]
    Whoami,

    #[command(name = LOGOUT_NAME, alias = LOGOUT_ALIAS, about = LOGOUT_ABOUT)]
    Logout,

    #[command(name = CONFIG_NAME, alias = CONFIG_ALIAS, about = CONFIG_ABOUT)]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Create a package.json file
    #[command(name = INIT_NAME, alias = INIT_ALIAS, about = INIT_ABOUT)]
    Init {
        /// Skip prompts and use defaults
        #[arg(long, short)]
        yes: bool,
    },

    /// Generate shell completion scripts
    #[command(name = COMPLETIONS_NAME, alias = COMPLETIONS_ALIAS, about = COMPLETIONS_ABOUT)]
    Completions {
        /// Shell to generate completions for (auto-detected if omitted)
        #[arg(value_enum)]
        shell: Option<clap_complete::Shell>,
    },
}

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
        if let Some(chain) = util::format_print::format_resolve_chain(&e) {
            tracing::error!("{:#}\n\n{chain}", e);
        } else {
            tracing::error!("{:#}", e);
        }
        if let Some(log_path) = get_log_file_path() {
            eprintln!("Full logs saved to: {}", log_path.display());
        }
        process::exit(1);
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
    if let Some(Commands::Install {
        from: Some(FromPm::Pnpm),
        ..
    }) = &cli.command
    {
        let cwd = std::env::current_dir()?;
        let root_path = init_project_root(&cwd).await?;
        migrate_from_pnpm(&root_path).await?;
    }

    // global registry
    init_registry(cli.registry).await;

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
        Some(Commands::Install {
            specs,
            workspace,
            ignore_scripts,
            save: _,
            save_dev,
            save_peer,
            save_optional,
            global,
            prefix,
            production,
            omit,
            from: _,
        }) => {
            // Build omit config: production = omit dev + optional
            let mut omit_set: std::collections::HashSet<OmitType> = omit.into_iter().collect();
            if production {
                omit_set.insert(OmitType::Dev);
                omit_set.insert(OmitType::Optional);
            }
            // legacy_peer_deps means omit peer
            if cli.legacy_peer_deps == Some(true) {
                omit_set.insert(OmitType::Peer);
            }
            set_omit(omit_set);

            if global {
                set_install_scope(InstallScope::Global);
            }

            if !specs.is_empty() {
                if global {
                    // For global installs, process packages one by one
                    for spec in specs.iter() {
                        install_global_package(spec, prefix.as_deref()).await?;
                    }
                    log_time_end(&pluralized_package_count(specs.len(), "installed"));
                } else {
                    let save_type = parse_save_type(save_dev, save_peer, save_optional);
                    let spec_refs: Vec<&str> = specs.iter().map(|s| s.as_str()).collect();
                    update_packages(
                        PackageAction::Add,
                        &spec_refs,
                        workspace.clone(),
                        ScriptPolicy::from(ignore_scripts),
                        save_type,
                    )
                    .await?;
                    // Log install result with correct singular/plural form in one line
                    log_time_end(&pluralized_package_count(specs.len(), "installed"));
                }
            } else {
                let cwd = std::env::current_dir()?;
                let root_path = init_project_root(&cwd).await?;
                install(ScriptPolicy::from(ignore_scripts), &root_path).await?;
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
                update_packages(
                    PackageAction::Remove,
                    &spec_refs,
                    workspace.clone(),
                    ScriptPolicy::from(ignore_scripts),
                    SaveType::Prod,
                )
                .await?;
                log_time_end(&pluralized_package_count(specs.len(), "uninstalled"));
            } else {
                anyhow::bail!("Package specification is required for uninstall");
            }
        }
        Some(Commands::Rebuild) => {
            let cwd = std::env::current_dir()?;
            rebuild(&cwd).await?;
            log_time_end("All packages rebuilt");
        }
        Some(Commands::Deps { workspace_only }) => {
            let cwd = std::env::current_dir()?;
            let root_path = init_project_root(&cwd).await?;
            if workspace_only {
                build_workspace(&root_path).await.map(|_| ())?
            } else {
                build_deps(&root_path).await.map(|_| ())? // Ignore returned PackageLock for CLI command
            };
            log_time_end("deps resolved");
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
            let script_args_owned = if args.is_empty() { None } else { Some(args) };
            let missing = if if_present {
                MissingScript::Skip
            } else {
                MissingScript::Fail
            };
            run(
                script.as_deref(),
                WorkspaceFilter::from_flags(workspace, workspaces),
                missing,
                script_args_owned,
            )
            .await?;
        }
        Some(Commands::View { package }) => {
            view(&package).await?;
        }
        Some(Commands::Link { packages, prefix }) => {
            let cwd = std::env::current_dir().context("Failed to get current directory")?;
            match packages {
                None => {
                    // Link current package to global
                    let package_name = link_current_to_global(&cwd, prefix.as_deref()).await?;
                    log_time_end(&format!("{package_name} linked"));
                }
                Some(packages) => {
                    for package in packages.iter() {
                        link_global_to_local(&cwd, package, prefix.as_deref()).await?;
                    }
                    log_time_end(&format!("'{}' linked to local", packages.join(", ")));
                }
            }
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
            cmd::publish::publish(tag.as_deref(), dry_run.into(), otp.as_deref()).await?;
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
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Set { key, value, global } => {
                handle_config_set(key, value, global.into()).await?;
            }
            ConfigCommands::Get {
                key,
                global,
                override_values,
            } => {
                handle_config_get(key, global.into(), override_values).await?;
            }
            ConfigCommands::List { global } => {
                handle_config_list(global.into()).await?;
            }
        },
        None => {
            // Check if there's a script name provided
            if let Some(script_name) = &cli.script_name {
                // First check if there's a custom command configured for this script name
                let config = crate::util::config_file::Config::load(ConfigScope::Local).await?;
                let config_service = crate::service::config::ConfigService::new(config);
                // Check if there's a custom command available
                if let Ok(Some(_)) = config_service.get_available_cmd(script_name) {
                    // Execute the custom command
                    config_service.execute_command(script_name, &cli.script_args)?;
                    return Ok(());
                }

                // If no custom command found, try to run as script
                let script_args_owned = if cli.script_args.is_empty() {
                    None
                } else {
                    Some(cli.script_args)
                };

                run(
                    Some(script_name.as_str()),
                    WorkspaceFilter::from_flags(cli.workspace, cli.workspaces),
                    MissingScript::Fail,
                    script_args_owned,
                )
                .await?;
            } else {
                // Default to install if no arguments
                let cwd = std::env::current_dir()?;
                let root_path = init_project_root(&cwd).await?;
                install(ScriptPolicy::from(cli.ignore_scripts), &root_path).await?;
                log_time_end("All packages installed");
            }
        }
        // Completions is handled early before initialization
        Some(Commands::Completions { .. }) => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_debug_assert() {
        // Validates that the clap command definition has no conflicts or issues
        Cli::command().debug_assert();
    }

    #[test]
    fn test_completions_generates_output() {
        for shell in [
            clap_complete::Shell::Bash,
            clap_complete::Shell::Zsh,
            clap_complete::Shell::Fish,
            clap_complete::Shell::PowerShell,
            clap_complete::Shell::Elvish,
        ] {
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), APP_NAME, &mut buf);
            let output = String::from_utf8(buf).expect("completion output should be valid UTF-8");
            assert!(
                !output.is_empty(),
                "{shell} completion should produce output"
            );
            assert!(
                output.contains("install"),
                "{shell} completion should contain subcommands"
            );
        }
    }

    #[test]
    fn test_install_add_alias_recognized() {
        // Verify clap correctly recognizes "add" as an alias for install
        let cmd = Cli::command();

        // Test "add" is recognized as install command
        let result = cmd.clone().try_get_matches_from(["utoo", "add", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo add' as valid Install command"
        );

        let matches = result.unwrap();
        assert_eq!(
            matches.subcommand_name(),
            Some("install"),
            "add alias should map to install subcommand"
        );
    }

    #[test]
    fn test_install_alias_still_works() {
        // Verify old alias "i" still works
        let cmd = Cli::command();

        let result = cmd.clone().try_get_matches_from(["utoo", "i", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo i' as valid Install command"
        );

        let matches = result.unwrap();
        assert_eq!(matches.subcommand_name(), Some("install"));
    }

    #[test]
    fn test_install_full_command_still_works() {
        // Verify full command name still works
        let cmd = Cli::command();

        let result = cmd.try_get_matches_from(["utoo", "install", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo install' as valid Install command"
        );
    }

    /// `utx --version` (= `utoo x --version`) must parse — the leading
    /// `--version`/`-v` lands in `command` rather than being rejected by clap —
    /// so `main` can print the version instead of erroring. (`--help`/`-h` is
    /// intercepted by clap's built-in help and is not exercised here.)
    #[test]
    fn test_execute_captures_leading_version_flag() {
        for flag in ["--version", "-v"] {
            let cli = Cli::try_parse_from(["utoo", "x", flag])
                .unwrap_or_else(|e| panic!("`utoo x {flag}` should parse, got: {e}"));
            match cli.command {
                Some(Commands::Execute { command, args }) => {
                    assert_eq!(command, flag);
                    assert!(args.is_empty());
                }
                _ => panic!("expected Execute subcommand"),
            }
        }
    }

    /// A flag after the package name is passed through to the package, not
    /// consumed as `x`'s own flag.
    #[test]
    fn test_execute_passes_through_package_flags() {
        let cli = Cli::try_parse_from(["utoo", "x", "cowsay", "--version"]).unwrap();
        match cli.command {
            Some(Commands::Execute { command, args }) => {
                assert_eq!(command, "cowsay");
                assert_eq!(args, vec!["--version".to_string()]);
            }
            _ => panic!("expected Execute subcommand"),
        }
    }
}
