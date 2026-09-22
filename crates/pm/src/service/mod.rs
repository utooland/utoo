//! Reusable project operations, installation, lifecycle and publication.
//!
//! `project` discovers roots, edits manifests, validates and persists locks.
//! `install` owns prefetch scheduling, lock-driven materialization, bins,
//! binary rewriting, dependency hooks and build-tool preparation.
//! `lifecycle` selects packages and orders stages; `script` only builds the
//! supplied npm environment, runs a child, captures output and waits for it.
//! `publish` owns packing, manifest rewriting, provenance and upload outcomes.
//!
//! Callers supply paths and policies. CLI parsing, process cwd changes,
//! handoff and exit decisions belong to `cmd`/`main`.

pub mod auth;
pub mod clean;
pub mod clean_cache;
pub mod config;
pub mod dependency_graph;
pub mod execute;
pub mod init;
pub mod install;
pub mod lifecycle;
pub mod oidc;
pub mod package_management;
pub mod publish;
pub mod script;
pub mod update;
pub mod workspace;

pub mod project;
mod workspace_builder;
