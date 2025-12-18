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
//! |   FileSystem     |  <- Platform abstraction (fs.rs)
//! | Tokio / OPFS     |
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
//!     fs: my_fs,
//!     receiver: my_receiver,
//! }).await?;
//! ```

mod api;
mod cache;
mod fs;
mod http;
mod registry;

pub use api::{BuildDepsOptions, build_deps};
pub use cache::{
    CacheStats, ManifestCache, MemoryCache, PackageCache, ProjectCache, ProjectCacheData,
    ProjectPackageCache, Versions, VersionsInfo, get_manifest_cache_path, get_versions_cache_path,
    load_project_cache, save_project_cache,
};
pub use fs::{FileSystem, Glob, NoopFileSystem};
pub use registry::UnifiedRegistry;
