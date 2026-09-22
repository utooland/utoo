//! # utoo-ruborist
//!
//! Rust Arborist - dependency resolution core for utoo package manager.
//!
//! This crate provides dependency resolution that works on both native CLI
//! and WebAssembly (browser) environments.
//!
//! ## Quick Start
//!
//! ```no_run
//! use std::path::PathBuf;
//! use utoo_ruborist::builder::PeerDeps;
//! use utoo_ruborist::progress::NoopReceiver;
//! use utoo_ruborist::service::{build_deps, read_root_manifest, BuildDepsOptions, NoopGlob, UnifiedRegistry};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let cwd = PathBuf::from("/project");
//! let (cwd, package) = read_root_manifest(&cwd, NoopGlob).await?;
//! let lock = build_deps(BuildDepsOptions {
//!     cwd,
//!     registry: UnifiedRegistry::builder().registry("https://registry.npmjs.org").build(),
//!     cache_dir: None,
//!     concurrency: 20,
//!     peer_deps: PeerDeps::Include,
//!     glob: NoopGlob, // Supply a Glob implementation for projects with workspaces.
//!     receiver: NoopReceiver,
//!     catalogs: Default::default(),
//!     baseline: None, // Supply a previously read PackageLock to reuse its layout.
//! }, package).await?;
//! let json = serde_json::to_string_pretty(&lock)?;
//! # Ok(())
//! # }
//! ```

pub mod model;
pub mod resolver;
pub mod service;
mod sources;
pub mod spec;
pub mod traits;
pub mod util;

// ============================================================================
// Re-exports: only items actually used by consumers
// ============================================================================

/// Dependency graph types.
pub mod graph {
    pub use crate::model::graph::DependencyGraph;
    pub use crate::model::node::EdgeType;
}

/// Package manifest types.
pub mod manifest {
    pub use crate::model::manifest::{
        CoreVersionManifest, Dist, FullManifest, VersionManifest, VersionsRef,
    };
    pub use crate::model::package_json::{
        BinField, IdentityView, PackageInstallView, PackageJson, PublishConfig, ScriptsView,
    };
}

/// Package lock types (package-lock.json).
pub mod lock {
    pub use crate::model::lock_codec::lock_satisfies_overrides;
    pub use crate::model::package_lock::{LockPackage, LockPackageNode, PackageLock};
}

/// Registry client types and version selection.
pub mod registry {
    pub use crate::resolver::registry::{ResolveError, resolve_package};
    pub use crate::resolver::version::resolve_target_version;
    pub use crate::traits::registry::{RegistryError, is_npm_registry};
}

/// Semver utilities.
pub mod semver {
    pub use crate::resolver::semver::matches;
}

/// Dependency resolution builder.
pub mod builder {
    pub use crate::model::node::{DevDeps, PeerDeps};
    pub use crate::resolver::builder::{add_workspace_member, resolve_workspace_member_edges};
    pub use crate::resolver::edges::{EdgeContext, add_edges_from};
}

/// Node.js runtime requirement helpers (engines field).
pub mod runtime {
    pub use crate::resolver::runtime::install_runtime_from_map;
}

/// Workspace member discovery.
pub mod workspace {
    pub use crate::resolver::workspace::WorkspaceDiscovery;
}

/// Progress events for build process.
pub mod progress {
    pub use crate::traits::progress::{
        BuildEvent, EventReceiver, NoopReceiver, PackageTarballInfo,
    };
}

/// Platform compatibility checks.
pub mod compat {
    pub use crate::model::compatibility::{
        PlatformConstraint, is_cpu_compatible, is_os_compatible, is_platform_compatible,
    };
}

/// Git clone and resolution helpers.
pub mod git {
    pub use crate::model::git::GitCloneResult;

    #[cfg(feature = "native-git")]
    pub use crate::sources::git::{GitCloneCache, ensure_repo_cached};
}

/// Tar + gzip primitives and the atomic cache-slot commit protocol.
///
/// Shared with pm's install-phase extractor
/// (`crates/pm/src/service/install/extract.rs`) so registry slots and BFS-seeded
/// slots are produced by identical gzip sizing, tar-slip guarding, and
/// mode normalization, under the same durability contract: every
/// `~/.cache/nm/` slot becomes visible only via atomic rename of a
/// fully-written staging directory that already contains the `_resolved`
/// marker (see `sources/common.rs`).
pub mod tar {
    #[cfg(any(feature = "native-git", feature = "http-tarball"))]
    pub use crate::sources::common::commit_cache_dir_atomic;
    #[cfg(feature = "http-tarball")]
    pub use crate::sources::tar::{
        MAX_UNCOMPRESSED_BYTES, estimate_uncompressed_size, extract_tarball_to_dir,
        gzip_decompress, is_safe_tar_entry_path, normalize_entry_mode,
    };
}
