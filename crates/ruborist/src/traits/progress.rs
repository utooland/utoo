//! Event system for build_deps progress and diagnostics.
//!
//! This module provides an event-driven approach for tracking dependency
//! resolution progress without adding any I/O to ruborist itself.

use std::path::Path;

pub use crate::model::tarball_info::PackageTarballInfo;

/// Events emitted during dependency resolution.
#[derive(Debug, Clone, Copy)]
pub enum BuildEvent<'a> {
    /// A package was fully resolved with download info.
    /// This event enables pipeline downloading - tarball can be downloaded
    /// immediately while other manifests are still being fetched.
    PackageResolved(PackageTarballInfo<'a>),

    /// Starting a new BFS level with N nodes to process.
    LevelStart { node_count: usize },

    /// Found N unresolved dependencies at current node.
    DependencyCount { count: usize },

    /// Starting to resolve a specific package.
    Resolving { name: &'a str },

    /// Successfully resolved a package (reused existing).
    Reused { name: &'a str, version: &'a str },

    /// Successfully resolved a package (created new node).
    Resolved { name: &'a str, version: &'a str },

    /// A package node was placed in the dependency tree.
    /// This enables pipeline cloning - the package can be cloned
    /// immediately after download, without waiting for full tree build.
    PackagePlaced {
        package: PackageTarballInfo<'a>,
        /// Target path in node_modules (e.g., "node_modules/lodash")
        path: &'a Path,
        /// Parent package path (None for root-level dependencies)
        parent_path: Option<&'a Path>,
    },

    /// Skipped an optional dependency.
    Skipped { name: &'a str, spec: &'a str },

    /// Completed a BFS level, next level has N nodes.
    LevelComplete { next_level_count: usize },

    /// Build completed successfully.
    Complete { total_nodes: usize },
}

/// Receiver for build events.
///
/// Implementations can handle events in any way they want:
/// - Display progress bars (CLI)
/// - Log messages
/// - Collect statistics
/// - Send to UI (WASM)
///
/// # Example
/// ```ignore
/// struct MyReceiver;
///
/// impl EventReceiver for MyReceiver {
///     fn on_event(&self, event: BuildEvent<'_>) {
///         match event {
///             BuildEvent::Resolving { name } => println!("Resolving {}...", name),
///             BuildEvent::Resolved { name, version } => println!("  {}@{}", name, version),
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait EventReceiver: Send + Sync {
    /// Called when a build event occurs.
    fn on_event(&self, event: BuildEvent<'_>);
}

/// A no-op event receiver for when event tracking is not needed.
pub struct NoopReceiver;

impl EventReceiver for NoopReceiver {
    fn on_event(&self, _event: BuildEvent<'_>) {}
}
