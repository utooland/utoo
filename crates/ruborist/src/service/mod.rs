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
//! ```ignore
//! use utoo_ruborist::service::{build_deps, BuildDepsOptions};
//!
//! let package_lock = build_deps(BuildDepsOptions {
//!     cwd: PathBuf::from("."),
//!     registry_url: "https://registry.npmmirror.com".to_string(),
//!     cache_dir: None,
//!     concurrency: 20,
//!     peer_deps: PeerDeps::Include,
//!     glob: my_glob,
//!     receiver: my_receiver,
//! }).await?;
//! ```

mod api;
mod cache;
pub(crate) mod dns;
pub(crate) mod fetch;
mod fs;
pub(crate) mod http;
pub(crate) mod manifest;
mod manifest_provider;
mod registry;
mod store;

pub use api::{BuildDepsOptions, build_deps, read_root_manifest};
pub use cache::{Versions, VersionsInfo};
pub use fs::{Glob, NoopGlob, exists, read_to_string};
pub use http::client_builder;
pub use manifest::{
    FetchManifestBytesResult, FetchManifestOptions, FetchManifestResult,
    FetchVersionManifestOptions, MetadataFormat, fetch_full_manifest, fetch_full_manifest_bytes,
    fetch_full_manifest_fresh,
};
pub use manifest_provider::{ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider};
pub use registry::UnifiedRegistry;
pub use store::{ManifestStore, NoopStore};
