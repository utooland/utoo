//! Demand-driven resolver, split by concern:
//!
//! - [`state`] — per-run manifest store (cache, waiters, failures).
//! - [`queue`] — fetch scheduling, single-flight de-duplication, priority.
//! - [`select`] — per-edge resolution decision (pure; reads the store).
//! - [`schedule`] — fetch-scheduling policy: which manifest jobs to enqueue.
//! - [`driver`] — BFS loop: drives fetches, applies results, builds the graph.
//!
//! `state`/`queue`/`select`/`schedule` are leaf concerns; the driver
//! coordinates them.

mod driver;
mod queue;
mod schedule;
mod select;
mod state;

pub(crate) use driver::run_main_loop_bfs;
pub(crate) use state::ManifestState;
