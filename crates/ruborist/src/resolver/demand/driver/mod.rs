//! Demand-driven BFS resolver driver, split by concern:
//!
//! - [`plan`] — per-edge resolution decisions over the store (pure, no I/O).
//! - [`schedule`] — what to enqueue (demand + speculative prefetch).
//! - [`pipeline`] — execute queued jobs and apply their results.
//! - [`run`] — the BFS loop that ties them together and mutates the graph.

mod pipeline;
mod plan;
mod run;
mod schedule;

pub(crate) use run::run_main_loop_bfs;
