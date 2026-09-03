//! Package-manager commands shared by the CLI and embedded hosts.
//!
//! Each module is one `utoo`/`ut` capability. Command handlers own the public
//! boundary and delegate business logic to the crate-private service layer.
//!
//! ```text
//!   CLI adapter ─┐
//!                ├──► commands::<capability> ──► crate::service::<logic>
//!   embedder ────┘
//! ```
//!
//! Representative routes: install/update → `service::install` ·
//! run → `service::script` · publish → `service::{publish,pm_pack}` ·
//! deps/list → `service::dependency_graph` · config → `service::config`.

pub mod clean;
pub mod completions;
pub mod config;
pub mod deps;
pub mod execute;
pub mod init;
pub mod install;
pub mod link;
pub mod list;
pub mod login;
pub mod logout;
pub mod pack;
pub mod ping;
pub mod publish;
pub mod rebuild;
pub mod run;
pub mod uninstall;
pub mod update;
pub mod view;
pub mod whoami;
