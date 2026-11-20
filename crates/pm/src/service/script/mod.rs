//! Script execution service.
//!
//! Split by responsibility:
//! - [`exec`] — command construction and execution primitives
//! - [`node_gyp`] — node-gyp bootstrap for native addon builds
//! - [`lifecycle`] — npm lifecycle orchestration (pre/post chains, workspaces)

mod exec;
mod lifecycle;
mod node_gyp;
#[cfg(windows)]
mod windows_shell;

pub(crate) use exec::script_failure_details;
pub use exec::{ScriptExit, ScriptService};
pub use lifecycle::{LifecycleSink, MachineLifecycleOutcome, MissingScript, ScriptOutput};
