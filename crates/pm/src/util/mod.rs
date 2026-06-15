//! Leaf utilities and runtime adapters — no business policy lives here.
//!
//! Two kinds: pure helpers (`json`, `integrity`, `format_print`, `cli_enum`,
//! `platform_const`, `cache::matches_pattern`) and runtime adapters that bind
//! process/network/disk state for [`crate::service`] and [`crate::helper`].
//!
//! ```text
//!   service:: / helper::  ──consume──►  util::
//!
//!     http · retry · downloader ───────────────► network (registry, tarballs)
//!     cloner · extractor · linker ─────────────► node_modules + ~/.cache/nm
//!     package_cache · project_cache ·
//!       manifest_store ────────────────────────► on-disk caches
//!     user_config · config_file ───────────────► ~/.utoo config + CLI flags
//!     git_resolver ──(binds cache dir)─────────► utoo_ruborist::git
//!     logger · sysconf · format_print ─────────► terminal / tracing
//! ```

pub mod cache;
pub mod cli_enum;
pub mod cloner;
pub mod config_file;
pub mod downloader;
pub mod extractor;
pub mod format_print;
pub mod git_resolver;
pub mod http;
pub mod integrity;
pub mod json;
pub mod linker;
pub mod logger;
pub mod manifest_store;
pub mod package_cache;
pub mod platform_const;
pub mod project_cache;
pub mod registry;
pub mod retry;
pub mod sysconf;
pub mod user_config;
