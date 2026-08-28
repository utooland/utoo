//! Embeddable API for the utoo package manager.
//!
//! The native `utoo` binary is a thin wrapper over [`run_cli`]. Hosts that
//! want package-manager behavior in-process should call the modules under
//! [`commands`] after configuring process-wide defaults through [`initialize`].

mod cli;
mod cli_entry;
pub mod commands;
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
pub use cli_entry::run as run_cli;
