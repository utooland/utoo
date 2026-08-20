//! Clap CLI definitions for the `utoo` binary.
//!
//! Pure argument-parsing types and helpers live here; per-command assembly
//! and execution belong to the owning `cmd::*` module.

use clap::{Parser, Subcommand};

use crate::cmd::install::InstallArgs;
use crate::cmd::update::UpdateArgs;
use crate::constants::cmd::{
    CLEAN_ABOUT, CLEAN_ALIAS, CLEAN_NAME, COMPLETIONS_ABOUT, COMPLETIONS_ALIAS,
    COMPLETIONS_LONG_ABOUT, COMPLETIONS_NAME, CONFIG_ABOUT, CONFIG_ALIAS, CONFIG_NAME, DEPS_ABOUT,
    DEPS_ALIAS, DEPS_NAME, EXECUTE_ABOUT, EXECUTE_ALIAS, EXECUTE_NAME, INIT_ABOUT, INIT_ALIAS,
    INIT_NAME, INSTALL_ABOUT, INSTALL_NAME, LINK_ABOUT, LINK_ALIAS, LINK_NAME, LIST_ABOUT,
    LIST_ALIAS, LIST_NAME, LOGIN_ABOUT, LOGIN_ALIAS, LOGIN_NAME, LOGOUT_ABOUT, LOGOUT_ALIAS,
    LOGOUT_NAME, PACK_ABOUT, PACK_ALIAS, PACK_NAME, PING_ABOUT, PING_ALIAS, PING_NAME,
    PUBLISH_ABOUT, PUBLISH_ALIAS, PUBLISH_NAME, REBUILD_ABOUT, REBUILD_ALIAS, REBUILD_NAME,
    RUN_ABOUT, RUN_ALIAS, RUN_NAME, UNINSTALL_ABOUT, UNINSTALL_ALIAS, UNINSTALL_NAME,
    UNINSTALL_REMOVE, UNINSTALL_RM, UPDATE_ABOUT, UPDATE_ALIAS, UPDATE_NAME, VIEW_ABOUT,
    VIEW_ALIAS, VIEW_ALIAS_INFO, VIEW_ALIAS_SHOW, VIEW_NAME, WHOAMI_ABOUT, WHOAMI_ALIAS,
    WHOAMI_NAME,
};
use crate::constants::{APP_ABOUT, APP_NAME, APP_VERSION};
use crate::util::cli_enum::PublishAccess;

pub fn detect_shell_from_env() -> Option<clap_complete::Shell> {
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
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(long)]
    pub ignore_scripts: bool,

    #[arg(short = 'v', long = "version")]
    pub version: bool,

    #[arg(long, global = true)]
    pub verbose: bool,

    /// Emit a single machine-readable JSON result
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-essential diagnostics
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Disable ANSI colors (also honors NO_COLOR)
    #[arg(long, global = true)]
    pub no_color: bool,

    #[arg(long, global = true)]
    pub registry: Option<String>,

    #[arg(long, global = true)]
    pub cache_dir: Option<String>,

    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    pub legacy_peer_deps: Option<bool>,

    /// Maximum concurrent manifest fetches (default: 64)
    #[arg(long, global = true)]
    pub manifests_concurrency_limit: Option<usize>,

    /// Maximum lifecycle scripts run at once (default: auto, ~CPU cores)
    #[arg(long, global = true)]
    pub script_concurrency_limit: Option<usize>,

    /// Workspace(s) to operate in by name, relative path, or glob pattern
    /// (repeatable). Also accepted as `--filter` for pnpm familiarity, e.g.
    /// `ut --filter @scope/pkg publish` targets a member from the workspace root.
    #[arg(long, global = true, hide = true, num_args = 1, alias = "filter")]
    pub workspace: Vec<String>,

    /// Run in all workspaces with topological ordering
    #[arg(long, global = true, hide = true, default_value = "false")]
    pub workspaces: bool,

    pub script_name: Option<String>,

    /// Arguments to pass to the script when running without explicit subcommand
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub script_args: Vec<String>,
}

impl Cli {
    /// Whether this invocation mutates or materializes the current project's
    /// dependency state and should therefore honor its `packageManager` pin.
    pub const fn uses_project_package_manager(&self) -> bool {
        if self.script_name.is_some() {
            return false;
        }
        match &self.command {
            None => true,
            Some(Commands::Install(args)) => !args.global,
            Some(
                Commands::Uninstall { .. }
                | Commands::Rebuild
                | Commands::Deps { .. }
                | Commands::Update(_),
            ) => true,
            Some(_) => false,
        }
    }
}

#[derive(Subcommand)]
pub enum ConfigCommands {
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
pub enum Commands {
    #[command(name = INSTALL_NAME, aliases = ["i", "add"], about = INSTALL_ABOUT)]
    Install(InstallArgs),

    #[command(name = UNINSTALL_NAME, aliases = [UNINSTALL_ALIAS,UNINSTALL_REMOVE,UNINSTALL_RM], about = UNINSTALL_ABOUT)]
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

        /// Skip the confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    #[command(name = DEPS_NAME, alias = DEPS_ALIAS, about = DEPS_ABOUT)]
    Deps {
        #[arg(long)]
        workspace_only: bool,
    },

    #[command(name = UPDATE_NAME, alias = UPDATE_ALIAS, about = UPDATE_ABOUT)]
    Update(UpdateArgs),

    #[command(name = LIST_NAME, alias = LIST_ALIAS, about = LIST_ABOUT)]
    List {
        /// Package name to show dependencies for
        #[arg(value_name = "PACKAGE")]
        package: String,
    },

    #[command(name = RUN_NAME, alias = RUN_ALIAS, about = RUN_ABOUT)]
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
        /// Registry visibility for the package: `public` or `restricted`
        /// (default: publishConfig.access, else public). Mirrors `npm --access`.
        #[arg(long, value_enum)]
        access: Option<PublishAccess>,
        /// Skip Git working-tree cleanliness checks. Accepted for pnpm
        /// compatibility; utoo performs no Git checks, so this is a no-op.
        #[arg(long)]
        no_git_checks: bool,
        /// Generate and attach a signed Sigstore/SLSA provenance attestation.
        /// Requires a supported CI (GitHub Actions / GitLab) with OIDC and a
        /// registry that exposes the attestation API.
        #[arg(long)]
        provenance: bool,
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

    #[command(name = INIT_NAME, alias = INIT_ALIAS, about = INIT_ABOUT)]
    Init {
        /// Skip prompts and use defaults
        #[arg(long, short)]
        yes: bool,
    },

    #[command(name = COMPLETIONS_NAME, alias = COMPLETIONS_ALIAS, about = COMPLETIONS_ABOUT, long_about = COMPLETIONS_LONG_ABOUT)]
    Completions {
        /// Shell to generate completions for (auto-detected if omitted)
        #[arg(value_enum)]
        shell: Option<clap_complete::Shell>,
    },
}

impl Commands {
    pub const fn json_name(&self) -> &'static str {
        match self {
            Self::Install(_) => "install",
            Self::Uninstall { .. } => "uninstall",
            Self::Rebuild => "rebuild",
            Self::Clean { .. } => "clean",
            Self::Deps { .. } => "deps",
            Self::Update(_) => "update",
            Self::List { .. } => "list",
            Self::Run { .. } => "run",
            Self::Execute { .. } => "execute",
            Self::View { .. } => "view",
            Self::Link { .. } => "link",
            Self::Pack { .. } => "pack",
            Self::Publish { .. } => "publish",
            Self::Ping { .. } => "ping",
            Self::Login => "login",
            Self::Whoami => "whoami",
            Self::Logout => "logout",
            Self::Config { .. } => "config",
            Self::Init { .. } => "init",
            Self::Completions { .. } => "completions",
        }
    }

    pub const fn json_subcommand(&self) -> Option<&'static str> {
        match self {
            Self::Config { command } => Some(command.json_name()),
            _ => None,
        }
    }
}

impl ConfigCommands {
    const fn json_name(&self) -> &'static str {
        match self {
            Self::Set { .. } => "set",
            Self::Get { .. } => "get",
            Self::List { .. } => "list",
        }
    }
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

    #[test]
    fn project_package_manager_scope_only_covers_local_dependency_commands() {
        for args in [
            vec!["utoo"],
            vec!["utoo", "install"],
            vec!["utoo", "add", "lodash"],
            vec!["utoo", "uninstall", "lodash"],
            vec!["utoo", "update"],
            vec!["utoo", "rebuild"],
            vec!["utoo", "deps"],
        ] {
            let cli = Cli::try_parse_from(args.clone()).unwrap();
            assert!(
                cli.uses_project_package_manager(),
                "expected {args:?} to use the project package manager"
            );
        }

        for args in [
            vec!["utoo", "install", "lodash", "--global"],
            vec!["utoo", "run", "test"],
            vec!["utoo", "x", "eslint"],
            vec!["utoo", "view", "react"],
            vec!["utoo", "test"],
        ] {
            let cli = Cli::try_parse_from(args.clone()).unwrap();
            assert!(
                !cli.uses_project_package_manager(),
                "expected {args:?} to keep using the running utoo"
            );
        }
    }

    #[test]
    fn test_update_force_flags() {
        for flag in ["--force", "-f"] {
            let cli = Cli::try_parse_from(["utoo", "update", flag])
                .unwrap_or_else(|e| panic!("`utoo update {flag}` should parse, got: {e}"));
            match cli.command {
                Some(Commands::Update(args)) => assert!(args.force),
                _ => panic!("expected Update subcommand"),
            }
        }

        let cli = Cli::try_parse_from(["utoo", "update"]).unwrap();
        match cli.command {
            Some(Commands::Update(args)) => assert!(!args.force),
            _ => panic!("expected Update subcommand"),
        }
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

    //test 'un','uninstall','remove','rm'
    #[test]
    fn test_install_un_alias_recognized() {
        // Verify clap correctly recognizes "un" as an alias for uninstall
        let cmd = Cli::command();

        // Test "un" is recognized as uninstall command
        let result = cmd.clone().try_get_matches_from(["utoo", "un", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo un' as valid uninstall command"
        );

        let matches = result.unwrap();
        assert_eq!(
            matches.subcommand_name(),
            Some("uninstall"),
            "un alias should map to uninstall subcommand"
        );
    }

    #[test]
    fn test_install_uninstall_alias_recognized() {
        // Verify clap correctly recognizes "uninstall" as an alias for uninstall
        let cmd = Cli::command();

        // Test "uninstall" is recognized as uninstall command
        let result = cmd
            .clone()
            .try_get_matches_from(["utoo", "uninstall", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo uninstall' as valid uninstall command"
        );

        let matches = result.unwrap();
        assert_eq!(
            matches.subcommand_name(),
            Some("uninstall"),
            "uninstall alias should map to uninstall subcommand"
        );
    }

    #[test]
    fn test_install_remove_alias_recognized() {
        // Verify clap correctly recognizes "remove" as an alias for uninstall
        let cmd = Cli::command();

        // Test "remove" is recognized as uninstall command
        let result = cmd
            .clone()
            .try_get_matches_from(["utoo", "remove", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo remove' as valid uninstall command"
        );

        let matches = result.unwrap();
        assert_eq!(
            matches.subcommand_name(),
            Some("uninstall"),
            "remove alias should map to uninstall subcommand"
        );
    }

    #[test]
    fn test_install_rm_alias_recognized() {
        // Verify clap correctly recognizes "rm" as an alias for uninstall
        let cmd = Cli::command();

        // Test "rm" is recognized as uninstall command
        let result = cmd.clone().try_get_matches_from(["utoo", "rm", "lodash"]);
        assert!(
            result.is_ok(),
            "Should parse 'utoo rm' as valid uninstall command"
        );

        let matches = result.unwrap();
        assert_eq!(
            matches.subcommand_name(),
            Some("uninstall"),
            "rm alias should map to uninstall subcommand"
        );
    }
}
