//! CLI subcommand handlers — the thin dispatch layer.
//!
//! Each module is one `utoo`/`ut` subcommand. It assembles parameters from the
//! parsed [`Cli`](crate::cli) and delegates to [`crate::service`]; no business
//! logic lives here (the `cmd/` layering rule in AGENTS.md).
//!
//! ```text
//!   main.rs ──(parse Cli, route on Commands)──► cmd::<subcommand>
//!                                                    │ assemble args,
//!                                                    │ map flags → enums
//!                                                    ▼
//!                                               crate::service::<logic>
//! ```
//!
//! Representative routes: install/update → `service::install` ·
//! run → `service::lifecycle` · publish/pack → `service::publish` ·
//! deps/list → `service::dependency_graph` · config → `service::config`.

pub mod clean;
pub mod config;
pub mod deps;
pub mod install;
pub mod link;
pub mod list;
pub mod login;
pub mod logout;
pub mod ping;
pub mod pm_pack;
pub mod publish;
pub mod rebuild;
pub mod run;
pub mod self_pin;
pub mod update;
pub mod view;
pub mod whoami;

pub mod help;
pub mod project;

/// A command's raw exit status; main owns the process exit after cleanup.
#[derive(Debug)]
pub(crate) struct CommandExit(pub i32);

impl std::fmt::Display for CommandExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "command exited with status {}", self.0)
    }
}
impl std::error::Error for CommandExit {}
