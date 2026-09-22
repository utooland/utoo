//! pm-local data types. The resolver's npm wire models (PackageJson,
//! manifests, lockfile) live in `utoo_ruborist`; this module holds only the
//! types pm itself owns.
//!
//! - `package` — `PackageInfo` / `LifecycleScripts` / lifecycle-hook types
//!   consumed by the install + script services
//! - `cli_output` — the stable machine-readable CLI output contract
//!
//! `RunMode` is re-exported from [`crate::util::cli_enum`] for convenience.

pub mod cli_output;
pub mod package;

pub use crate::util::cli_enum::RunMode;
