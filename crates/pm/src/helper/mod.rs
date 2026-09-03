//! Cross-cutting orchestration helpers between [`crate::commands`]/[`crate::service`]
//! and the `utoo_ruborist` resolver — workspace topology, lockfile lifecycle,
//! and dep-graph utilities that don't belong to a single service.
//!
//! ```text
//!   ruborist_context ─ adapt pm config/auth/glob ─► utoo_ruborist::build_deps
//!   lock             ─ ensure/regenerate/save package-lock.json,
//!                       edit package.json, resolve specs
//!   tree_builder     ─ workspace-only topology graph (no network)
//!   workspace        ─ find/chdir to project & workspace roots
//!   deps             ─ SCC cycle groups + topological layers (petgraph)
//!   migrate          ─ import pnpm/npm lockfiles into utoo
//!   global_bin · git · fuzzy_select · auto_update
//! ```

pub mod auto_update;
pub mod deps;
pub mod fuzzy_select;
pub mod git;
pub mod global_bin;
pub mod lock;
pub mod migrate;
pub mod ruborist_context;
pub mod self_pin;
pub mod tree_builder;
pub mod workspace;
