//! Event system for build_deps progress and diagnostics.
//!
//! This module provides an event-driven approach for tracking dependency
//! resolution progress without adding any I/O to ruborist itself.

/// Events emitted during dependency resolution.
#[derive(Debug, Clone)]
pub enum BuildEvent {
    /// Starting preload phase with N initial dependencies.
    PreloadStart { count: usize },

    /// More dependencies were discovered and queued for preloading.
    PreloadQueued { count: usize },

    /// A fetch task was started for a package.
    PreloadFetching { name: String },

    /// A package was preloaded successfully.
    PreloadProgress {
        name: String,
        version: String,
        /// Current count of preloaded packages
        current: usize,
    },

    /// Preload phase completed with success/failed counts.
    PreloadComplete { success: usize, failed: usize },

    /// Starting a new BFS level with N nodes to process.
    LevelStart { node_count: usize },

    /// Found N unresolved dependencies at current node.
    DependencyCount { count: usize },

    /// Starting to resolve a specific package.
    Resolving { name: String },

    /// Successfully resolved a package (reused existing).
    Reused { name: String, version: String },

    /// Successfully resolved a package (created new node).
    Resolved { name: String, version: String },

    /// Skipped an optional dependency.
    Skipped { name: String, spec: String },

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
///     fn on_event(&self, event: BuildEvent) {
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
    fn on_event(&self, event: BuildEvent);
}

/// A no-op event receiver for when event tracking is not needed.
pub struct NoopReceiver;

impl EventReceiver for NoopReceiver {
    fn on_event(&self, _event: BuildEvent) {}
}
