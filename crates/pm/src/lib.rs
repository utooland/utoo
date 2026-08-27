//! Embeddable API for the utoo package manager.
//!
//! The native `utoo` binary is a thin wrapper over [`cli_main`]. Hosts that
//! want package-manager behavior in-process should call the modules under
//! [`commands`] after configuring process-wide defaults through [`initialize`].

mod cli;
mod cli_entry;
mod cmd;
mod constants;
pub mod error;
mod fs;
mod helper;
mod model;
mod service;
mod util;

use std::path::PathBuf;

use anyhow::Result;
use clap::CommandFactory;

/// utoo package-manager version.
pub const VERSION: &str = constants::APP_VERSION;

/// Build the complete clap command tree used by the native binary.
pub fn command() -> clap::Command {
    cli::Cli::command()
}

/// Public command modules corresponding to the subcommands exposed by the
/// `utoo` binary.
///
/// Each module keeps its existing typed entry points; command-line parsing,
/// process exit handling, logging setup, and automatic updates remain in the
/// binary adapter.
pub mod commands {
    /// Generate shell-completion scripts, equivalent to `utoo completions`.
    pub mod completions {
        use anyhow::Context;

        /// Generate a UTF-8 completion script for `shell`.
        pub fn generate(shell: clap_complete::Shell) -> anyhow::Result<String> {
            let mut output = Vec::new();
            let mut command = crate::command();
            clap_complete::generate(shell, &mut command, crate::constants::APP_NAME, &mut output);
            String::from_utf8(output).context("Generated completion script is not UTF-8")
        }
    }

    pub mod clean {
        pub async fn run(
            pattern: &str,
            confirmation: crate::types::ConfirmationPolicy,
        ) -> anyhow::Result<()> {
            crate::cmd::clean::clean(pattern, confirmation).await
        }
    }

    pub mod config {
        pub use crate::cli::ConfigCommands as Command;

        pub async fn run(command: Command) -> anyhow::Result<()> {
            crate::cmd::config::run(command).await
        }
    }

    pub mod deps {
        pub async fn run(workspace_only: bool) -> anyhow::Result<()> {
            crate::cmd::deps::run(workspace_only).await
        }
    }

    pub mod execute {
        pub async fn run(command: &str, args: Vec<String>) -> anyhow::Result<()> {
            crate::service::execute::execute_package(command, args).await
        }
    }

    pub mod init {
        use std::path::PathBuf;

        pub async fn run(
            mode: crate::types::InitMode,
            output: crate::types::InitOutput,
            project_dir: Option<PathBuf>,
        ) -> anyhow::Result<()> {
            crate::service::init::init(mode, output, project_dir.as_deref()).await
        }
    }

    pub mod install {
        use std::path::Path;

        pub use crate::cmd::install::InstallArgs as Options;
        pub use crate::helper::migrate::FromPm;

        pub async fn run(options: Options, legacy_peer_deps: Option<bool>) -> anyhow::Result<()> {
            crate::cmd::install::run(options, legacy_peer_deps).await
        }

        pub async fn project(
            project_dir: &Path,
            scripts: crate::types::ScriptPolicy,
        ) -> anyhow::Result<()> {
            crate::cmd::install::install(scripts, project_dir).await
        }

        pub async fn current_project(scripts: crate::types::ScriptPolicy) -> anyhow::Result<()> {
            crate::cmd::install::install_cwd(scripts).await
        }

        pub async fn global(package: &str, prefix: Option<&str>) -> anyhow::Result<()> {
            crate::cmd::install::install_global_package(package, prefix).await
        }

        pub async fn migrate_from(source: Option<FromPm>) -> anyhow::Result<()> {
            crate::cmd::install::migrate_from(source).await
        }
    }

    pub mod link {
        pub async fn run(
            packages: Option<Vec<String>>,
            prefix: Option<String>,
        ) -> anyhow::Result<()> {
            crate::cmd::link::run(packages, prefix).await
        }
    }

    pub mod list {
        use std::path::Path;

        pub async fn run(project_dir: &Path, package: &str) -> anyhow::Result<()> {
            crate::cmd::list::list_dependencies(project_dir, package).await
        }
    }

    pub mod login {
        pub async fn run() -> anyhow::Result<()> {
            crate::cmd::login::login().await
        }
    }

    pub mod logout {
        pub async fn run() -> anyhow::Result<()> {
            crate::cmd::logout::logout().await
        }
    }

    pub mod pack {
        pub async fn run(path: Option<String>, mode: crate::types::RunMode) -> anyhow::Result<()> {
            crate::cmd::pm_pack::pack(path, mode).await
        }
    }

    pub mod ping {
        pub async fn run(registry: Option<&str>) -> anyhow::Result<()> {
            crate::cmd::ping::ping(registry).await
        }
    }

    pub mod publish {
        pub async fn run(
            tag: Option<&str>,
            mode: crate::types::RunMode,
            otp: Option<&str>,
            access: Option<crate::types::PublishAccess>,
            filter: crate::types::WorkspaceFilter,
            provenance: crate::types::ProvenancePolicy,
        ) -> anyhow::Result<()> {
            crate::cmd::publish::publish(tag, mode, otp, access, filter, provenance).await
        }
    }

    pub mod rebuild {
        use std::path::Path;

        pub async fn run(project_dir: &Path) -> anyhow::Result<()> {
            crate::cmd::rebuild::rebuild(project_dir).await
        }
    }

    pub mod run {
        pub async fn run(
            script: Option<&str>,
            filter: crate::types::WorkspaceFilter,
            missing: crate::types::MissingScript,
            args: Option<Vec<String>>,
        ) -> anyhow::Result<()> {
            crate::cmd::run::run(script, filter, missing, args).await
        }

        pub async fn fallback(
            name: &str,
            filter: crate::types::WorkspaceFilter,
            args: Vec<String>,
        ) -> anyhow::Result<()> {
            crate::cmd::run::run_fallback(name, filter, args).await
        }
    }

    pub mod uninstall {
        pub async fn run(
            packages: Vec<String>,
            workspace: Option<String>,
            scripts: crate::types::ScriptPolicy,
        ) -> anyhow::Result<()> {
            crate::cmd::install::uninstall(packages, workspace, scripts).await
        }
    }

    pub mod update {
        pub use crate::cmd::update::UpdateArgs as Options;

        pub async fn run(
            options: Options,
            scripts: crate::types::ScriptPolicy,
        ) -> anyhow::Result<()> {
            crate::cmd::update::update(options, scripts).await
        }
    }

    pub mod view {
        pub async fn run(package: &str) -> anyhow::Result<()> {
            crate::cmd::view::view(package).await
        }
    }

    pub mod whoami {
        pub async fn run() -> anyhow::Result<()> {
            crate::cmd::whoami::whoami().await
        }
    }
}

/// Public types used by command entry points.
pub mod types {
    pub use crate::cli::ConfigCommands;
    pub use crate::helper::migrate::{FromPm, MigrateResult};
    pub use crate::model::cli_output::*;
    pub use crate::service::init::InitOutput;
    pub use crate::service::script::{MissingScript, ScriptExit, ScriptOutput};
    pub use crate::service::workspace::{WorkspaceFilter, WorkspaceJson};
    pub use crate::util::cli_enum::{
        ConfigScope, ConfirmationPolicy, InitMode, InstallScope, OmitType, PackageAction,
        ProvenancePolicy, PublishAccess, ReifyMode, RunMode, SaveType, ScriptPolicy,
    };
    pub use clap_complete::Shell;
}

/// Process-wide defaults used by the command layer.
///
/// utoo currently stores these settings once per process. Call [`initialize`]
/// before the first command when the host needs values other than the built-in
/// defaults. Repeated initialization with different values is not supported.
#[derive(Debug, Clone, Default)]
pub struct InitializeOptions {
    /// Registry URL override. Configuration and environment defaults are used
    /// when omitted.
    pub registry: Option<String>,
    /// Package cache directory override.
    pub cache_dir: Option<PathBuf>,
    /// Whether peer dependencies should be skipped.
    pub legacy_peer_deps: Option<bool>,
    /// Maximum number of concurrent manifest requests.
    pub manifests_concurrency_limit: Option<usize>,
    /// Maximum number of concurrent lifecycle scripts.
    pub script_concurrency_limit: Option<usize>,
}

/// Initialize process-wide package-manager defaults for embedded use.
pub async fn initialize(options: InitializeOptions) -> Result<()> {
    util::sysconf::init();
    util::user_config::init_registry(options.registry).await?;
    util::user_config::set_cache_dir(
        options
            .cache_dir
            .map(|path| path.to_string_lossy().into_owned()),
    )
    .await;
    util::user_config::set_legacy_peer_deps(options.legacy_peer_deps);
    util::user_config::set_manifests_concurrency_limit(options.manifests_concurrency_limit);
    util::user_config::set_script_concurrency_limit(options.script_concurrency_limit);
    Ok(())
}

/// Run the full native command-line application.
pub use cli_entry::run as cli_main;
