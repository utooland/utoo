//! Unified service layer for dependency resolution.
//!
//! This module provides a unified API that works on both native (CLI) and
//! WASM (browser) environments.
//!
//! # Architecture
//!
//! ```text
//! +------------------+
//! |    build_deps    |  <- High-level API (api.rs)
//! +------------------+
//!          |
//!          v
//! +------------------+
//! | UnifiedRegistry  |  <- Registry client (registry.rs)
//! +------------------+
//!     |         |
//!     v         v
//! +-------+ +--------+
//! | Cache | | HTTP   |
//! +-------+ +--------+
//!          |
//!          v
//! +------------------+
//! |  tokio-fs-ext    |  <- Platform abstraction
//! +------------------+
//! ```
//!
//! # Usage
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

mod api;
pub(crate) mod dns;
pub(crate) mod fetch;
mod fs;
pub(crate) mod http;
mod registry;

pub use api::{BuildDepsOptions, build_deps, build_deps_with_root_dev_deps, read_root_manifest};
pub use fs::{Glob, NoopGlob, exists, read_to_string};
pub use http::client_builder;
pub use registry::UnifiedRegistry;
pub use registry::cache::{Versions, VersionsInfo};
pub use registry::manifest::{
    FetchManifestBytesResult, FetchManifestOptions, FetchManifestResult,
    FetchVersionManifestOptions, MetadataFormat, fetch_full_manifest, fetch_full_manifest_bytes,
    fetch_full_manifest_fresh,
};
pub use registry::manifest_provider::{
    ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider,
};
pub use registry::store::{ManifestStore, NoopStore};
