//! Demand-driven resolver, split by concern:
//!
//! - [`state`] — per-run manifest store (cache, waiters, failures).
//! - [`queue`] — fetch scheduling, single-flight de-duplication, priority.
//!
//! The two are independent; the driver that coordinates them (and exposes
//! `run_main_loop_bfs`) lands in the follow-up PR. Until then they are staged
//! here and exercised by their own unit tests.

// Staged ahead of the driver: the only consumers so far are the unit tests, so
// the driver-facing API is dead in this PR.
#![allow(dead_code)]

mod queue;
mod state;
