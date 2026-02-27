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
//!     legacy_peer_deps: false,
//!     glob: my_glob,
//!     receiver: my_receiver,
//! }).await?;
//! ```

mod api;
mod cache;
pub(crate) mod dns;
mod fs;
mod http;
mod manifest;
mod registry;

pub use api::{BuildDepsOptions, build_deps};
pub use cache::{
    CacheStats, ManifestCache, MemoryCache, PackageCache, ProjectCache, ProjectCacheData,
    ProjectPackageCache, Versions, VersionsInfo, get_manifest_cache_path, get_versions_cache_path,
    load_project_cache, save_project_cache,
};
pub use fs::{Glob, NoopGlob, exists, read_to_string};
pub use http::client_builder;
pub use manifest::{FetchManifestOptions, MetadataFormat, fetch_full_manifest};
pub use registry::UnifiedRegistry;
