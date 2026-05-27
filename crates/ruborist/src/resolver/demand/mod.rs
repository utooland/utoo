//! Demand-driven resolver, split by concern:
//!
//! - [`state`] — per-run manifest store (cache, waiters, failures).
//! - [`queue`] — fetch scheduling, single-flight de-duplication, priority.
//! - [`driver`] — walks the dependency graph and coordinates the store + queue.
//!
//! `state` and `queue` are independent; only the driver knows about both.

mod driver;
mod queue;
mod state;

pub(crate) use driver::run_main_loop_bfs;
pub(crate) use state::ResolverManifestCache;
