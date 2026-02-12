//! Dependency resolution module.
//!
//! Uses ruborist's unified `build_deps` API with OPFS file system.

use anyhow::Result;
use std::path::PathBuf;

use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::progress::NoopReceiver;
use utoo_ruborist::service::{build_deps, BuildDepsOptions};

use crate::fs::OpfsGlob;

/// Default registry URL.
const DEFAULT_REGISTRY: &str = "https://registry.npmmirror.com";

/// Default concurrency for browser environment.
const DEFAULT_CONCURRENCY: usize = 20;

/// Build dependency lock from package.json in the given directory.
///
/// Uses ruborist's unified API which provides:
/// - Automatic registry capability detection
/// - Three-tier caching (memory -> OPFS disk -> network)
/// - Support for both npm and npmmirror-style registries
///
/// # Arguments
/// * `cwd` - Current working directory containing package.json
/// * `registry_url` - Optional registry URL (defaults to npmmirror)
/// * `concurrency` - Optional concurrency limit (defaults to 20)
pub async fn build_deps_from_file(
    cwd: &std::path::Path,
    registry_url: Option<&str>,
    concurrency: Option<usize>,
) -> Result<PackageLock> {
    let options = BuildDepsOptions {
        cwd: PathBuf::from(cwd),
        registry_url: registry_url.unwrap_or(DEFAULT_REGISTRY).to_string(),
        cache_dir: None,
        concurrency: concurrency.unwrap_or(DEFAULT_CONCURRENCY),
        legacy_peer_deps: true,
        glob: OpfsGlob,
        receiver: NoopReceiver,
        supports_semver: None,
    };

    build_deps(options).await
}
