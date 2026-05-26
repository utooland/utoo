//! Demand-driven resolver, split by concern:
//!
//! - [`state`] — per-run manifest store (cache, waiters, failures).
//!
//! The fetch scheduler ([`queue`]) and the driver that coordinates them land in
//! the follow-up PRs; until then the store is staged here and exercised by its
//! own unit tests.

// Staged ahead of the driver: the only consumers so far are the unit tests, so
// the driver-facing API is dead in this PR.
#![allow(dead_code)]

mod state;
