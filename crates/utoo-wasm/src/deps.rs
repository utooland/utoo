//! Dependency resolution module.
//!
//! Uses ruborist's unified `build_deps` API with OPFS file system.

use std::sync::Arc;

use anyhow::Result;
use utoo_ruborist::builder::PeerDeps;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::progress::NoopReceiver;
use utoo_ruborist::service::{
    build_deps, read_root_manifest, BuildDepsOptions, NoopStore, UnifiedRegistry,
};

use crate::fs::OpfsGlob;

/// Default registry URL.
const DEFAULT_REGISTRY: &str = "https://registry.npmmirror.com";

/// Default concurrency for browser environment.
const DEFAULT_CONCURRENCY: usize = 20;

/// Build dependency lock from package.json in the given directory.
///
/// Uses ruborist's unified API. Persistence is opt-in: the browser build
/// uses [`NoopStore`] (no manifest cache), so every cold resolve hits the
/// network. The host can swap in an OPFS-backed store later.
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
    // Resolve the workspace root + read its manifest from OPFS, then resolve.
    let (root_path, pkg) = read_root_manifest(cwd, OpfsGlob).await?;
    // Browser builds have no private-registry auth (no env/config token source),
    // so the registry is constructed without a token.
    let options = BuildDepsOptions {
        cwd: root_path,
        registry: UnifiedRegistry::builder()
            .registry(registry_url.unwrap_or(DEFAULT_REGISTRY))
            .store(Arc::new(NoopStore))
            .build(),
        cache_dir: None,
        project_cache: None,
        concurrency: concurrency.unwrap_or(DEFAULT_CONCURRENCY),
        peer_deps: PeerDeps::Skip,
        glob: OpfsGlob,
        receiver: NoopReceiver,
        catalogs: std::collections::HashMap::new(),
    };

    build_deps(options, pkg).await.map(|output| output.lock)
}
