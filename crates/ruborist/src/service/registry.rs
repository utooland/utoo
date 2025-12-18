//! Unified registry client implementation.
//!
//! Provides `UnifiedRegistry` that works on both native and WASM targets.
//! Combines HTTP fetching with in-memory caching and automatic registry
//! capability detection (semver support).
//!
//! For non-semver registries (npmjs.org), supports disk cache with ETag validation:
//! - `versions.json`: version list + dist-tags + etag (lightweight)
//! - `manifests/{version}.json`: individual version manifests
//!
//! # Architecture
//!
//! - `http` module: Pure HTTP requests (`fetch_full_manifest`, `fetch_version_manifest`)
//! - `UnifiedRegistry`: Handles caching logic with three-tier strategy
//!   - Memory cache (fastest)
//!   - Disk cache (persistent, with ETag validation)
//!   - Network (authoritative source)

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;

/// Get current timestamp in seconds since UNIX epoch.
/// Works on both native and WASM targets.
#[cfg(not(target_arch = "wasm32"))]
fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Get current timestamp in seconds since UNIX epoch.
/// Uses js_sys::Date for WASM targets.
#[cfg(target_arch = "wasm32")]
fn current_timestamp_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

use super::cache::{PackageCache, Versions, VersionsInfo};
use super::fs::FileSystem;
use super::http;
use crate::model::manifest::{FullManifest, VersionManifest};
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::traits::registry::{RegistryClient, RegistryError, ResolvedPackage, is_npm_registry};

/// Unified registry client that works on both native and WASM.
///
/// Uses three-tier caching:
/// 1. Memory cache (fastest, lost on restart)
/// 2. Disk cache (persistent, requires FileSystem)
/// 3. Network (slowest, always authoritative)
///
/// For non-semver registries (npmjs.org), uses ETag validation
/// to avoid re-downloading unchanged manifests.
///
/// # Example
///
/// ```ignore
/// // Using builder pattern
/// let registry = UnifiedRegistry::builder(fs)
///     .registry("https://registry.npmmirror.com")
///     .cache_dir(PathBuf::from("/tmp/cache"))
///     .build();
/// ```
pub struct UnifiedRegistry<FS> {
    registry_url: String,
    cache: Arc<PackageCache>,
    fs: FS,
    supports_semver: bool,
}

/// Builder for `UnifiedRegistry`.
pub struct UnifiedRegistryBuilder<FS> {
    fs: FS,
    registry_url: Option<String>,
    cache: Option<Arc<PackageCache>>,
    cache_dir: Option<PathBuf>,
}

impl<FS: FileSystem> UnifiedRegistryBuilder<FS> {
    /// Create a new builder with the given file system.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            registry_url: None,
            cache: None,
            cache_dir: None,
        }
    }

    /// Set the registry URL.
    pub fn registry(mut self, url: impl Into<String>) -> Self {
        self.registry_url = Some(url.into());
        self
    }

    /// Set the cache directory for disk caching.
    pub fn cache_dir(mut self, path: PathBuf) -> Self {
        self.cache_dir = Some(path);
        self
    }

    /// Set a shared cache instance.
    pub fn cache(mut self, cache: Arc<PackageCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Build the registry client.
    pub fn build(self) -> UnifiedRegistry<FS> {
        let registry_url = self
            .registry_url
            .unwrap_or_else(|| "https://registry.npmmirror.com".to_string());
        let supports_semver = !is_npm_registry(&registry_url);

        // Priority: shared cache > cache_dir > new cache
        let cache = self.cache.unwrap_or_else(|| {
            if let Some(dir) = self.cache_dir {
                Arc::new(PackageCache::with_cache_dir(dir))
            } else {
                Arc::new(PackageCache::new())
            }
        });

        UnifiedRegistry {
            registry_url,
            cache,
            fs: self.fs,
            supports_semver,
        }
    }
}

impl<FS: Clone> Clone for UnifiedRegistry<FS> {
    fn clone(&self) -> Self {
        Self {
            registry_url: self.registry_url.clone(),
            cache: Arc::clone(&self.cache),
            fs: self.fs.clone(),
            supports_semver: self.supports_semver,
        }
    }
}

impl<FS: FileSystem> UnifiedRegistry<FS> {
    /// Create a builder for `UnifiedRegistry`.
    pub fn builder(fs: FS) -> UnifiedRegistryBuilder<FS> {
        UnifiedRegistryBuilder::new(fs)
    }

    /// Get the registry URL.
    pub fn registry_url(&self) -> &str {
        &self.registry_url
    }

    /// Check if this registry supports semver resolution.
    pub fn supports_semver(&self) -> bool {
        self.supports_semver
    }

    /// Get the underlying cache.
    pub fn cache(&self) -> &PackageCache {
        &self.cache
    }

    /// Resolve full manifest with three-tier caching and ETag validation.
    ///
    /// Cache flow:
    /// 1. Check memory cache -> return if hit
    /// 2. Check disk cache for versions.json (etag + version list)
    /// 3. Fetch from network with etag for 304 validation
    /// 4. On 304: use disk cache data
    /// 5. On 200: update memory + disk cache
    async fn resolve_full_manifest(&self, name: &str) -> Result<FullManifest, RegistryError> {
        // 1. Check memory cache first
        if let Some(manifest) = self.cache.get_full_manifest(name) {
            tracing::debug!("Memory cache hit for full manifest: {}", name);
            return Ok(manifest);
        }

        // 2. Load etag from disk cache (if available)
        let disk_versions = self.cache.get_versions_from_disk(&self.fs, name).await;
        let etag = disk_versions.as_ref().and_then(|v| v.etag.clone());

        // 3. Fetch from network with ETag for validation
        let use_abbreviated = self.supports_semver;
        match http::fetch_full_manifest(&self.registry_url, name, use_abbreviated, etag.as_deref())
            .await
        {
            Ok((manifest, new_etag)) => {
                // 4. Cache full manifest in memory
                self.cache
                    .set_full_manifest(name.to_string(), manifest.clone());

                // 5. Extract and cache versions info (lightweight)
                let versions_info = VersionsInfo {
                    versions: Versions {
                        version_list: manifest.versions.keys().cloned().collect(),
                        dist_tags: manifest.dist_tags.clone(),
                    },
                    etag: new_etag.clone(),
                    last_updated: current_timestamp_secs(),
                };
                self.cache
                    .set_versions(name.to_string(), versions_info.clone());

                // 6. Write versions to disk (non-blocking for native, blocking for WASM)
                self.cache
                    .set_versions_to_disk(&self.fs, name, &versions_info)
                    .await;

                Ok(manifest)
            }
            Err(e) if e.to_string().contains("Not modified") => {
                // 304 Not Modified - use disk cache versions info
                tracing::debug!("ETag cache hit (304) for: {}", name);

                if let Some(versions_info) = disk_versions {
                    // Cache versions in memory for later use
                    self.cache
                        .set_versions(name.to_string(), versions_info.clone());

                    // Return a placeholder FullManifest with just versions info
                    // The actual version manifests will be fetched individually when needed
                    // This avoids fetching the entire full manifest again
                    Err(RegistryError(anyhow!(
                        "304 Not Modified - use versions cache for {}",
                        name
                    )))
                } else {
                    // Disk cache corrupted, fetch fresh
                    let (manifest, new_etag) =
                        http::fetch_full_manifest(&self.registry_url, name, use_abbreviated, None)
                            .await
                            .map_err(RegistryError)?;

                    self.cache
                        .set_full_manifest(name.to_string(), manifest.clone());

                    let versions_info = VersionsInfo {
                        versions: Versions {
                            version_list: manifest.versions.keys().cloned().collect(),
                            dist_tags: manifest.dist_tags.clone(),
                        },
                        etag: new_etag.clone(),
                        last_updated: current_timestamp_secs(),
                    };
                    self.cache
                        .set_versions(name.to_string(), versions_info.clone());
                    self.cache
                        .set_versions_to_disk(&self.fs, name, &versions_info)
                        .await;

                    Ok(manifest)
                }
            }
            Err(e) => Err(RegistryError(e)),
        }
    }

    /// Resolve version manifest with three-tier caching.
    ///
    /// Cache key is `name@spec` (e.g., `lodash@^4.17.0`).
    /// This allows cache hits when the same spec is requested multiple times.
    ///
    /// Cache flow:
    /// 1. Memory cache -> fastest
    /// 2. Disk cache -> persistent
    /// 3. Network -> authoritative
    async fn resolve_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<VersionManifest, RegistryError> {
        // 1. Check memory cache using name@spec as key
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            tracing::debug!("Memory cache hit for version manifest: {}@{}", name, spec);
            return Ok(manifest);
        }

        // 2. Check disk cache (only for non-semver registries)
        // Semver registries resolve specs server-side, so disk cache keys (name@spec)
        // may not match the actual resolved version.
        if !self.supports_semver
            && let Some(manifest) = self
                .cache
                .get_version_manifest_from_disk(&self.fs, name, spec)
                .await
        {
            tracing::debug!("Disk cache hit for version manifest: {}@{}", name, spec);
            // Already cached in memory by get_version_manifest_from_disk
            return Ok(manifest);
        }

        // 3. Fetch from network (http module handles pure HTTP)
        // Use abbreviated format only for semver-supporting registries
        tracing::debug!("Cache miss for {}@{}, fetching from network", name, spec);
        let manifest =
            http::fetch_version_manifest(&self.registry_url, name, spec, self.supports_semver)
                .await
                .map_err(RegistryError)?;

        // 4. Cache in memory
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest.clone());

        // 5. Write to disk cache (only for non-semver registries)
        if !self.supports_semver {
            self.cache
                .set_version_manifest_to_disk(&self.fs, name, spec, &manifest)
                .await;
        }

        Ok(manifest)
    }
}

impl<FS: FileSystem> RegistryClient for UnifiedRegistry<FS> {
    type Error = RegistryError;

    fn supports_semver_resolution(&self) -> bool {
        self.supports_semver
    }

    fn get_cached_full_manifest(&self, name: &str) -> Option<FullManifest> {
        self.cache.get_full_manifest(name)
    }

    fn get_cached_versions(&self, name: &str) -> Option<crate::traits::registry::VersionsInfo> {
        self.cache
            .get_versions(name)
            .map(|v| crate::traits::registry::VersionsInfo {
                version_list: v.versions.version_list,
                dist_tags: v.versions.dist_tags,
            })
    }

    fn cache_version_manifest(&self, name: &str, spec: &str, manifest: VersionManifest) {
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest);
    }

    async fn fetch_full_manifest(&self, name: &str) -> Result<FullManifest, Self::Error> {
        self.resolve_full_manifest(name).await
    }

    async fn fetch_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<VersionManifest, Self::Error> {
        // 1. Check memory cache first
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            tracing::debug!("Memory cache hit for version manifest: {}@{}", name, spec);
            return Ok(manifest);
        }

        // 2. Check disk cache (only for non-semver registries where spec is exact version)
        if !self.supports_semver
            && let Some(manifest) = self
                .cache
                .get_version_manifest_from_disk(&self.fs, name, spec)
                .await
        {
            tracing::debug!("Disk cache hit for version manifest: {}@{}", name, spec);
            // Cache in memory for next time
            self.cache
                .set_version_manifest(name.to_string(), spec.to_string(), manifest.clone());
            return Ok(manifest);
        }

        // 3. Fetch from network
        // Both semver and non-semver registries support {registry}/{name}/{version}
        // Use abbreviated format only for semver-supporting registries
        let manifest =
            http::fetch_version_manifest(&self.registry_url, name, spec, self.supports_semver)
                .await
                .map_err(RegistryError)?;

        // 4. Cache the result
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest.clone());

        // 5. Write to disk cache (only for non-semver registries)
        if !self.supports_semver {
            self.cache
                .set_version_manifest_to_disk(&self.fs, name, spec, &manifest)
                .await;
        }

        Ok(manifest)
    }

    async fn resolve_package(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<ResolvedPackage, Self::Error> {
        // Normalize spec (handles npm: alias and workspace: prefix)
        let (fetch_name, fetch_spec) = normalize_spec(name, spec);
        if fetch_name != name || fetch_spec != spec {
            tracing::debug!(
                "Normalized {}@{} -> {}@{}",
                name,
                spec,
                fetch_name,
                fetch_spec
            );
        }

        // Always check memory cache first (project cache is pre-loaded here)
        if let Some(manifest) = self.cache.get_version_manifest(&fetch_name, &fetch_spec) {
            tracing::debug!(
                "Memory cache hit for version manifest: {}@{}",
                fetch_name,
                fetch_spec
            );
            return Ok(ResolvedPackage {
                name: name.to_string(),
                version: manifest.version.clone(),
                manifest,
            });
        }

        // Check full manifest cache (from preload phase)
        // This avoids redundant HTTP requests when preload already fetched the full manifest
        if let Some(full_manifest) = self.cache.get_full_manifest(&fetch_name) {
            tracing::debug!(
                "Full manifest cache HIT for {}@{}, resolving from cached manifest",
                fetch_name,
                fetch_spec
            );
            tracing::debug!(
                "Using cached full manifest for {}@{}",
                fetch_name,
                fetch_spec
            );
            let version_list: Vec<String> = full_manifest.versions.keys().cloned().collect();
            let resolved_version =
                resolve_target_version(&full_manifest.dist_tags, &version_list, &fetch_spec)
                    .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
            let version_manifest = full_manifest
                .versions
                .get(&resolved_version)
                .cloned()
                .ok_or_else(|| {
                    RegistryError(anyhow!(
                        "Version {} not found in manifest for {}",
                        resolved_version,
                        fetch_name
                    ))
                })?;
            // Cache version_manifest for project cache export
            self.cache.set_version_manifest(
                fetch_name.to_string(),
                fetch_spec.to_string(),
                version_manifest.clone(),
            );
            // Write to disk cache for non-semver registries
            if !self.supports_semver {
                self.cache
                    .set_version_manifest_to_disk(
                        &self.fs,
                        &fetch_name,
                        &resolved_version,
                        &version_manifest,
                    )
                    .await;
            }
            return Ok(ResolvedPackage {
                name: name.to_string(),
                version: resolved_version,
                manifest: version_manifest,
            });
        }

        if self.supports_semver {
            // Semver-supporting registry: use cached version manifest
            tracing::debug!("Using semver resolution for {}@{}", fetch_name, fetch_spec);
            let manifest = self
                .resolve_version_manifest(&fetch_name, &fetch_spec)
                .await?;
            Ok(ResolvedPackage {
                name: name.to_string(),
                version: manifest.version.clone(),
                manifest,
            })
        } else {
            // 1. Try to resolve using cached versions (avoid network request)
            if let Some(versions_info) = self.cache.get_versions(&fetch_name)
                && let Ok(resolved_version) = resolve_target_version(
                    &versions_info.versions.dist_tags,
                    &versions_info.versions.version_list,
                    &fetch_spec,
                )
            {
                // Check if we have the version manifest cached
                if let Some(manifest) = self
                    .cache
                    .get_version_manifest(&fetch_name, &resolved_version)
                {
                    tracing::debug!(
                        "Using cached versions + manifest for {}@{} => {}",
                        fetch_name,
                        fetch_spec,
                        resolved_version
                    );
                    return Ok(ResolvedPackage {
                        name: name.to_string(),
                        version: resolved_version,
                        manifest,
                    });
                }

                // Have versions cache but not version manifest, fetch it
                if let Ok(manifest) = self
                    .resolve_version_manifest(&fetch_name, &resolved_version)
                    .await
                {
                    tracing::debug!(
                        "Using cached versions, fetched manifest for {}@{} => {}",
                        fetch_name,
                        fetch_spec,
                        resolved_version
                    );
                    // Cache for project cache export
                    self.cache.set_version_manifest(
                        fetch_name.to_string(),
                        fetch_spec.to_string(),
                        manifest.clone(),
                    );
                    return Ok(ResolvedPackage {
                        name: name.to_string(),
                        version: resolved_version,
                        manifest,
                    });
                }
            }

            // Try to get full manifest, handle 304 case specially
            let resolve_result = match self.resolve_full_manifest(&fetch_name).await {
                Ok(full_manifest) => {
                    // Got full manifest, resolve from it
                    let version_list: Vec<String> =
                        full_manifest.versions.keys().cloned().collect();

                    if version_list.is_empty() {
                        return Err(RegistryError(anyhow!(
                            "No versions available for {}",
                            fetch_name
                        )));
                    }

                    let resolved_version = resolve_target_version(
                        &full_manifest.dist_tags,
                        &version_list,
                        &fetch_spec,
                    )
                    .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;

                    let version_manifest = full_manifest
                        .versions
                        .get(&resolved_version)
                        .cloned()
                        .ok_or_else(|| {
                        RegistryError(anyhow!(
                            "Version {} not found in manifest for {}",
                            resolved_version,
                            fetch_name
                        ))
                    })?;

                    (resolved_version, version_manifest)
                }
                Err(e) if e.0.to_string().contains("304 Not Modified") => {
                    // 304 case: use versions cache to resolve, then fetch version manifest
                    tracing::debug!("Using versions cache for {}@{}", fetch_name, fetch_spec);

                    let versions_info = self.cache.get_versions(&fetch_name).ok_or_else(|| {
                        RegistryError(anyhow!("Versions cache not found for {}", fetch_name))
                    })?;

                    let resolved_version = resolve_target_version(
                        &versions_info.versions.dist_tags,
                        &versions_info.versions.version_list,
                        &fetch_spec,
                    )
                    .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;

                    // Fetch individual version manifest
                    let version_manifest = self
                        .resolve_version_manifest(&fetch_name, &resolved_version)
                        .await?;

                    (resolved_version, version_manifest)
                }
                Err(e) => return Err(e),
            };

            let (resolved_version, version_manifest) = resolve_result;

            // Cache in memory for project cache export
            self.cache.set_version_manifest(
                fetch_name.to_string(),
                fetch_spec.to_string(),
                version_manifest.clone(),
            );

            Ok(ResolvedPackage {
                name: name.to_string(),
                version: resolved_version,
                manifest: version_manifest,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::NoopFileSystem;

    #[test]
    fn test_is_npm_registry() {
        assert!(is_npm_registry("https://registry.npmjs.org"));
        assert!(is_npm_registry("https://registry.npmjs.com"));
        assert!(!is_npm_registry("https://registry.npmmirror.com"));
        assert!(!is_npm_registry("https://registry.yarnpkg.com"));
    }

    #[test]
    fn test_unified_registry_builder() {
        // Default registry (npmmirror)
        let registry = UnifiedRegistry::builder(NoopFileSystem).build();
        assert!(registry.supports_semver());
        assert_eq!(registry.registry_url(), "https://registry.npmmirror.com");

        // Custom registry
        let registry = UnifiedRegistry::builder(NoopFileSystem)
            .registry("https://registry.npmjs.org")
            .build();
        assert!(!registry.supports_semver());
        assert_eq!(registry.registry_url(), "https://registry.npmjs.org");
    }

    #[test]
    fn test_unified_registry_with_cache_dir() {
        let registry = UnifiedRegistry::builder(NoopFileSystem)
            .registry("https://registry.npmmirror.com")
            .cache_dir(PathBuf::from("/tmp/cache"))
            .build();
        assert!(registry.supports_semver());
        assert!(registry.cache().cache_dir().is_some());
    }

    #[test]
    fn test_unified_registry_with_shared_cache() {
        let shared_cache = Arc::new(PackageCache::with_cache_dir(PathBuf::from("/tmp/shared")));

        let registry1 = UnifiedRegistry::builder(NoopFileSystem)
            .registry("https://registry.npmmirror.com")
            .cache(Arc::clone(&shared_cache))
            .build();

        let registry2 = UnifiedRegistry::builder(NoopFileSystem)
            .registry("https://registry.npmmirror.com")
            .cache(Arc::clone(&shared_cache))
            .build();

        // Both registries share the same cache
        assert!(Arc::ptr_eq(&registry1.cache, &registry2.cache));
    }
}
