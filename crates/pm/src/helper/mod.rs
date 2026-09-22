//! Supporting operations: migration, graph ordering, global paths, Git
//! metadata, selection prompts, self-update checks and self-pin preparation.
//! Project discovery and persistence live in `service::project`; process
//! handoff belongs to `cmd::self_pin`.

pub mod auto_update;
pub mod deps;
pub mod fuzzy_select;
pub mod git;
pub mod global_bin;
pub mod migrate;
pub mod self_pin;
