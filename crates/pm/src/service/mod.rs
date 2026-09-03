//! Business-logic layer — the work behind each CLI command.
//!
//! [`crate::commands`] assembles args and calls in here; this layer orchestrates
//! [`crate::util`] (I/O, cache, linker, http) and `utoo_ruborist` (resolution).
//!
//! ```text
//!   install pipeline:
//!     install ─► dependency_graph ─► install_scheduler ─► package ─► rebuild
//!     (omit/flags)  (lock via         (bounded download/    (collect +
//!                    ruborist)          extract/clone)        run lifecycle)
//!                                            │                    │
//!                                         binary            script (exec/
//!                                    (mirror env+rewrite)    node_gyp/lifecycle)
//!
//!   publish:    publish ─► publish_manifest ─► pm_pack ─► auth · oidc
//!   workspace:  workspace · init · clean · clean_cache · update · execute
//!               · package_management (utx)
//!   config:     config (+util::config_file) · auth · oidc
//! ```

pub mod auth;
pub mod binary;
pub mod clean;
pub mod clean_cache;
pub mod config;
pub mod dependency_graph;
pub mod execute;
pub mod init;
pub mod install;
pub mod install_scheduler;
pub mod oidc;
pub mod package;
pub mod package_management;
pub mod pm_pack;
pub mod provenance;
pub mod publish;
pub mod publish_manifest;
pub mod rebuild;
pub mod script;
pub mod update;
pub mod workspace;
