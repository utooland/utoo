//! Event system for build_deps progress and diagnostics.
//!
//! This module provides an event-driven approach for tracking dependency
//! resolution progress without adding any I/O to ruborist itself.

use std::path::Path;

use crate::model::manifest::VersionManifest;

/// Package tarball information for downloading.
///
/// A lightweight structure containing only the fields needed for
/// downloading and verifying a package tarball. Uses references to
/// avoid cloning data from the source manifest.
#[derive(Debug, Clone, Copy)]
pub struct PackageTarballInfo<'a> {
    /// Package name
    pub name: &'a str,
    /// Resolved version
    pub version: &'a str,
    /// Tarball URL for downloading
    pub tarball_url: Option<&'a str>,
    /// Integrity hash for verification
    pub integrity: Option<&'a str>,
    /// OS compatibility constraint (if specified)
    pub os: Option<&'a serde_json::Value>,
    /// CPU compatibility constraint (if specified)
    pub cpu: Option<&'a serde_json::Value>,
}

impl PackageTarballInfo<'_> {
    /// Check if this package is compatible with the current platform (os + cpu).
    pub fn is_platform_compatible(&self) -> bool {
        use crate::compat::{is_cpu_compatible, is_os_compatible};
        if let Some(os) = self.os {
            if !is_os_compatible(os) {
                return false;
            }
        }
        if let Some(cpu) = self.cpu {
            if !is_cpu_compatible(cpu) {
                return false;
            }
        }
        true
    }
}

impl<'a> From<&'a VersionManifest> for PackageTarballInfo<'a> {
    fn from(m: &'a VersionManifest) -> Self {
        Self {
            name: &m.name,
            version: &m.version,
            tarball_url: m.dist.tarball.as_deref(),
            integrity: m.dist.integrity.as_deref(),
            os: m.os.as_ref(),
            cpu: m.cpu.as_ref(),
        }
    }
}

/// Events emitted during dependency resolution.
#[derive(Debug, Clone, Copy)]
pub enum BuildEvent<'a> {
    /// Starting preload phase with N initial dependencies.
    PreloadStart { count: usize },

    /// More dependencies were discovered and queued for preloading.
    PreloadQueued { count: usize },

    /// A fetch task was started for a package.
    PreloadFetching { name: &'a str },

    /// A package was preloaded successfully.
    PreloadProgress {
        name: &'a str,
        version: &'a str,
        /// Current count of preloaded packages
        current: usize,
    },

    /// A package was fully resolved with download info.
    /// This event enables pipeline downloading - tarball can be downloaded
    /// immediately while other manifests are still being fetched.
    PackageResolved(PackageTarballInfo<'a>),

    /// Preload phase completed with success/failed counts.
    PreloadComplete { success: usize, failed: usize },

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
