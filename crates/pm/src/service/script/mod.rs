//! npm environment construction, single-process execution and output capture.
//! Lifecycle selection and ordering live in `service::lifecycle`; build tools
//! are prepared by install/pack/publish.
mod exec;
pub(crate) use exec::{OutputSink, ScriptFailure, script_failure_details, status_exit_code};
pub use exec::{PreparedTools, ScriptEnvironment, ScriptExit, ScriptService};

/// How script output is handled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutput {
    /// Stream to terminal in real time (user-facing scripts).
    Verbose,
    /// Capture and only print on failure (dependency lifecycle scripts).
    Silent,
    /// Capture without writing to stdout/stderr (machine invocations).
    Machine,
}
