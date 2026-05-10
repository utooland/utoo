//! Dependency resolution module.
//!
//! Uses ruborist's unified `build_deps` API with OPFS file system.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use utoo_ruborist::builder::PeerDeps;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::progress::NoopReceiver;
use utoo_ruborist::service::{build_deps, BuildDepsOptions, NoopStore};

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
    let options = BuildDepsOptions {
        cwd: PathBuf::from(cwd),
        registry_url: registry_url.unwrap_or(DEFAULT_REGISTRY).to_string(),
        cache_dir: None,
        manifest_store: Arc::new(NoopStore),
        warm_project_cache: None,
        concurrency: concurrency.unwrap_or(DEFAULT_CONCURRENCY),
        peer_deps: PeerDeps::Skip,
        glob: OpfsGlob,
        receiver: NoopReceiver,
        supports_semver: None,
        catalogs: std::collections::HashMap::new(),
        // wasm32 stays on the legacy preload + BFS path (channel
        // mb_fetch_with_graph requires multi-thread tokio + Send,
        // both unavailable on wasm).
        skip_preload: false,
    };

    build_deps(options).await.map(|output| output.lock)
}
