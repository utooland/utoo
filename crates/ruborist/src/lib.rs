//! # utoo-ruborist
//!
//! Rust Arborist - dependency resolution core for utoo package manager.
//!
//! This crate provides dependency resolution that works on both native CLI
//! and WebAssembly (browser) environments.
//!
//! ## Quick Start
//!
//! ```ignore
//! use utoo_ruborist::service::{build_deps, BuildDepsOptions, NoopFileSystem};
//! use utoo_ruborist::progress::NoopReceiver;
//!
//! let package_lock = build_deps(BuildDepsOptions {
//!     cwd: PathBuf::from("."),
//!     registry_url: "https://registry.npmmirror.com".to_string(),
//!     cache_dir: None,
//!     concurrency: 20,
//!     peer_deps: PeerDeps::Include,
//!     fs: NoopFileSystem,
//!     receiver: NoopReceiver,
//! }).await?;
//! ```

pub mod model;
pub mod resolver;
pub mod service;
pub mod spec;
pub mod traits;
pub mod util;

// ============================================================================
// Re-exports: only items actually used by consumers
// ============================================================================

/// Dependency graph types.
pub mod graph {
    pub use crate::model::graph::{DependencyGraph, PackageNode};
    pub use crate::model::node::EdgeType;
}

/// Package manifest types.
pub mod manifest {
    pub use crate::model::manifest::{
        CoreVersionManifest, Dist, FullManifest, VersionManifest, VersionsRef,
    };
    pub use crate::model::package_json::{
        DepsView, EnginesView, IdentityView, PackageInstallView, PackageJson, PublishConfig,
        ScriptsView,
    };
}

/// Package lock types (package-lock.json).
pub mod lock {
    pub use crate::model::package_lock::{LockPackage, LockPackageNode, PackageLock};
}

/// Registry client types.
pub mod registry {
    pub use crate::resolver::registry::resolve_package;
    pub use crate::traits::registry::is_npm_registry;
}

/// Semver utilities.
pub mod semver {
    pub use crate::resolver::semver::matches;
}

/// Dependency resolution builder.
pub mod builder {
    pub use crate::model::node::{DevDeps, PeerDeps};
    pub use crate::resolver::builder::add_workspace_member;
    pub use crate::resolver::edges::{DependencySource, EdgeContext, add_edges_from};
}

/// Progress events for build process.
pub mod progress {
    pub use crate::traits::progress::{
        BuildEvent, EventReceiver, NoopReceiver, PackageTarballInfo,
    };
}

/// Platform compatibility checks.
pub mod compat {
    pub use crate::model::compatibility::{is_cpu_compatible, is_os_compatible};
}

/// Node types for the dependency graph.
pub mod node {
    pub use crate::model::node::NodeType;
}

/// Git clone and resolution helpers.
pub mod git {
    pub use crate::model::git::GitCloneResult;

    #[cfg(feature = "native-git")]
    pub use crate::resolver::git::{GitCloneCache, ensure_repo_cached};
}

/// Non-registry tarball cache-slot helpers (http + local file).
pub mod http {
    #[cfg(feature = "http-tarball")]
    pub use crate::resolver::http::{file_cache_slot, http_cache_slot};
}
