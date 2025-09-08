// Re-export from the new service modules
pub use crate::service::cache::{store_cache, load_cache, flush_cache_to_disk, PACKAGE_CACHE};
pub use crate::service::registry::{resolve, resolve_dependency, ResolvedPackage};
pub use crate::service::http_client::get_package_info;

// Optimized caching for resolved versions
pub async fn cache_resolved_version(name: &str, version: &str, manifest: &serde_json::Value) {
    PACKAGE_CACHE.cache_version_manifest(name, version, manifest).await;
}
