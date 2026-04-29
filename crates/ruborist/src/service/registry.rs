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
//! - `manifest` module: Manifest fetching with retry (`fetch_full_manifest`, `fetch_version_manifest`)
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
use super::manifest;
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::traits::registry::{RegistryClient, RegistryError, ResolvedPackage, is_npm_registry};
#[cfg(not(target_arch = "wasm32"))]
use crate::util::oncemap::OnceMap;

/// Unified registry client that works on both native and WASM.
///
/// Uses three-tier caching:
/// 1. Memory cache (fastest, lost on restart)
/// 2. Disk cache (persistent, via tokio-fs-ext)
/// 3. Network (slowest, always authoritative)
///
/// For non-semver registries (npmjs.org), uses ETag validation
/// to avoid re-downloading unchanged manifests.
///
/// # Example
///
/// ```ignore
/// // Using builder pattern
/// let registry = UnifiedRegistry::builder()
///     .registry("https://registry.npmmirror.com")
///     .cache_dir(PathBuf::from("/tmp/cache"))
///     .build();
/// ```
pub struct UnifiedRegistry {
    registry_url: String,
    cache: Arc<PackageCache>,
    supports_semver: bool,
    /// Dedupes concurrent `resolve_full_manifest` fetches for the same
    /// package name. First caller hits the network and stores the result;
    /// other callers wait on `Notify` and read the shared `Arc`. Built on
    /// `DashMap` + `tokio::sync::Notify` so the fast path (cache hit) is
    /// lock-free, avoiding the serialisation the previous per-name
    /// `tokio::sync::Mutex<()>` gate imposed on the hot dispatch path.
    #[cfg(not(target_arch = "wasm32"))]
    inflight: Arc<OnceMap<String, FullManifestResult>>,
}

/// Builder for `UnifiedRegistry`.
pub struct UnifiedRegistryBuilder {
    registry_url: Option<String>,
    cache: Option<Arc<PackageCache>>,
    cache_dir: Option<PathBuf>,
    supports_semver: Option<bool>,
}

impl UnifiedRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry_url: None,
            cache: None,
            cache_dir: None,
            supports_semver: None,
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

    /// Explicitly set whether the registry supports semver resolution.
    ///
    /// If not set, defaults to `!is_npm_registry(url)`.
    pub fn supports_semver(mut self, value: bool) -> Self {
        self.supports_semver = Some(value);
        self
    }

    /// Build the registry client.
    pub fn build(self) -> UnifiedRegistry {
        let registry_url = self
            .registry_url
            .unwrap_or_else(|| "https://registry.npmmirror.com".to_string());
        let supports_semver = self
            .supports_semver
            .unwrap_or_else(|| !is_npm_registry(&registry_url));

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
            supports_semver,
            #[cfg(not(target_arch = "wasm32"))]
            inflight: Arc::new(OnceMap::new()),
        }
    }
}

impl Default for UnifiedRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for UnifiedRegistry {
    fn clone(&self) -> Self {
        Self {
            registry_url: self.registry_url.clone(),
            cache: Arc::clone(&self.cache),
            supports_semver: self.supports_semver,
            #[cfg(not(target_arch = "wasm32"))]
            inflight: Arc::clone(&self.inflight),
        }
    }
}

/// Result of `resolve_full_manifest`.
///
/// Separates the 200 (full data) and 304 (use cache) cases at the type level,
/// so callers can pattern-match instead of string-matching error messages.
/// Transient return value, immediately destructured — Box not needed.
///
/// `Clone` is required so multiple `resolve_full_manifest` callers that
/// coalesce through `OnceMap` can each take an owned copy of the shared
/// `Arc<FullManifestResult>`. `Arc<FullManifest>` keeps that clone an
/// atomic-bump rather than a deep copy of the per-version `OwnedValue`
/// HashMap (~100-500 entries per package).
#[derive(Clone)]
enum FullManifestResult {
    /// Fresh manifest fetched from the network (HTTP 200).
    Full(Arc<FullManifest>),
    /// ETag matched, versions cache is valid (HTTP 304).
    /// Caller should resolve from the in-memory versions cache and
    /// fetch individual version manifests as needed.
    NotModified,
}

impl UnifiedRegistry {
    /// Create a builder for `UnifiedRegistry`.
    pub fn builder() -> UnifiedRegistryBuilder {
        UnifiedRegistryBuilder::new()
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
    async fn resolve_full_manifest(&self, name: &str) -> Result<FullManifestResult, RegistryError> {
        // Fast path: memory cache hit — lock-free read from parking_lot::RwLock.
        if let Some(manifest) = self.cache.get_full_manifest(name) {
            tracing::debug!("Memory cache hit for full manifest: {}", name);
            return Ok(FullManifestResult::Full(manifest));
        }

        // Coalesce concurrent callers for the same name via OnceMap.
        // First caller runs the fetch closure; others await the shared
        // result on the OnceMap's `Notify` and clone the cached value.
        // OnceMap is gated to native targets (wasm has no parallel callers
        // worth coalescing — see `util/mod.rs`).
        #[cfg(not(target_arch = "wasm32"))]
        {
            let shared = self
                .inflight
                .get_or_init(name.to_string(), || async {
                    self.fetch_full_manifest_network(name).await.ok()
                })
                .await;

            match shared {
                Some(arc) => Ok((*arc).clone()),
                None => {
                    // OnceMap clears the key on None, so the next caller
                    // retries the fetch. Retry once here with a fresh error
                    // so we surface a useful message to this caller.
                    self.fetch_full_manifest_network(name).await
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.fetch_full_manifest_network(name).await
        }
    }

    /// Perform the actual network fetch + cache update. Separated from
    /// `resolve_full_manifest` so the OnceMap closure can invoke it
    /// without re-entering the dedup layer.
    async fn fetch_full_manifest_network(
        &self,
        name: &str,
    ) -> Result<FullManifestResult, RegistryError> {
        // Disk ETag probe. The `PackageCache::get_versions_from_disk`
        // call first consults a bulk-readdir index built lazily on
        // first access — cold runs with an empty (or nonexistent)
        // cache_dir short-circuit without per-package syscalls, which
        // was the cold-path regression the earlier temporary removal
        // (46cb8031) was meant to avoid. Warm runs pay the read +
        // JSON parse once per previously-cached manifest and reuse
        // its ETag to get a 1-packet `304 Not Modified` instead of
        // re-downloading the full manifest body.
        let disk_versions = self.cache.get_versions_from_disk(name).await;
        let etag = disk_versions.as_ref().and_then(|v| v.etag.clone());

        match manifest::fetch_full_manifest(manifest::FetchManifestOptions {
            registry_url: &self.registry_url,
            name,
            format: manifest::MetadataFormat::Abbreviated,
            etag: etag.as_deref(),
        })
        .await
        .map_err(RegistryError)?
        {
            manifest::FetchManifestResult::Ok(manifest, new_etag) => {
                let versions_info = VersionsInfo {
                    versions: Versions {
                        version_list: manifest.versions.keys.clone(),
                        dist_tags: manifest.dist_tags.clone(),
                    },
                    etag: new_etag.clone(),
                    last_updated: current_timestamp_secs(),
                };
                let manifest = Arc::new(manifest);
                self.cache
                    .set_full_manifest(name.to_string(), Arc::clone(&manifest));
                self.cache
                    .set_versions(name.to_string(), versions_info.clone());
                // Fire-and-forget disk cache write.
                self.cache.set_versions_to_disk(name, &versions_info);

                Ok(FullManifestResult::Full(manifest))
            }
            manifest::FetchManifestResult::NotModified => {
                tracing::debug!("ETag cache hit (304) for: {}", name);

                if let Some(versions_info) = disk_versions {
                    self.cache
                        .set_versions(name.to_string(), versions_info.clone());
                    Ok(FullManifestResult::NotModified)
                } else {
                    // Disk cache disappeared between index build and now
                    // (mid-run eviction or concurrent cleanup). Fall
                    // back to a fresh fetch without etag.
                    let (manifest, new_etag) = manifest::fetch_full_manifest_fresh(
                        &self.registry_url,
                        name,
                        manifest::MetadataFormat::Abbreviated,
                    )
                    .await
                    .map_err(RegistryError)?;

                    let versions_info = VersionsInfo {
                        versions: Versions {
                            version_list: manifest.versions.keys.clone(),
                            dist_tags: manifest.dist_tags.clone(),
                        },
                        etag: new_etag.clone(),
                        last_updated: current_timestamp_secs(),
                    };
                    let manifest = Arc::new(manifest);
                    self.cache
                        .set_full_manifest(name.to_string(), Arc::clone(&manifest));
                    self.cache
                        .set_versions(name.to_string(), versions_info.clone());
                    self.cache.set_versions_to_disk(name, &versions_info);

                    Ok(FullManifestResult::Full(manifest))
                }
            }
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
    ) -> Result<Arc<CoreVersionManifest>, RegistryError> {
        // 1. Check memory cache using name@spec as key
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            tracing::debug!("Memory cache hit for version manifest: {}@{}", name, spec);
            return Ok(manifest);
        }

        // 2. Check disk cache (only for non-semver registries)
        // Semver registries resolve specs server-side, so disk cache keys (name@spec)
        // may not match the actual resolved version.
        if !self.supports_semver
            && let Some(manifest) = self.cache.get_version_manifest_from_disk(name, spec).await
        {
            tracing::debug!("Disk cache hit for version manifest: {}@{}", name, spec);
            // Already cached in memory by get_version_manifest_from_disk
            return Ok(manifest);
        }

        // 3. Fetch from network (http module handles pure HTTP)
        // Use abbreviated format only for semver-supporting registries
        tracing::debug!("Cache miss for {}@{}, fetching from network", name, spec);
        let manifest = manifest::fetch_version_manifest(manifest::FetchVersionManifestOptions {
            registry_url: &self.registry_url,
            name,
            spec,
            format: if self.supports_semver {
                manifest::MetadataFormat::Abbreviated
            } else {
                manifest::MetadataFormat::Complete
            },
        })
        .await
        .map_err(RegistryError)?;

        let manifest = Arc::new(manifest);

        // 4. Cache in memory
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest.clone());

        // 5. Write to disk cache (only for non-semver registries)
        if !self.supports_semver {
            self.cache
                .set_version_manifest_to_disk(name, spec, manifest.clone());
        }

        Ok(manifest)
    }
}

impl RegistryClient for UnifiedRegistry {
    type Error = RegistryError;

    fn supports_semver_resolution(&self) -> bool {
        self.supports_semver
    }

    fn get_cached_full_manifest(&self, name: &str) -> Option<FullManifest> {
        // Trait still returns owned for backward compat with non-resolver
        // callers (notably `ut view`). Resolver hot paths read the
        // `Arc<FullManifest>` directly via `cache.get_full_manifest`.
        self.cache.get_full_manifest(name).map(|arc| (*arc).clone())
    }

    fn get_cached_versions(&self, name: &str) -> Option<crate::traits::registry::VersionsInfo> {
        self.cache
            .get_versions(name)
            .map(|v| crate::traits::registry::VersionsInfo {
                version_list: v.versions.version_list,
                dist_tags: v.versions.dist_tags,
            })
    }

    fn cache_version_manifest(&self, name: &str, spec: &str, manifest: Arc<CoreVersionManifest>) {
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest);
    }

    async fn fetch_full_manifest(&self, name: &str) -> Result<FullManifest, Self::Error> {
        match self.resolve_full_manifest(name).await? {
            FullManifestResult::Full(manifest) => Ok((*manifest).clone()),
            FullManifestResult::NotModified => {
                // 304 in trait context: caller doesn't have versions cache access,
                // so we return an error indicating the manifest is unchanged.
                Err(RegistryError(anyhow!(
                    "No versions available for {} (304 Not Modified but no full manifest cached)",
                    name
                )))
            }
        }
    }

    async fn fetch_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<Arc<CoreVersionManifest>, Self::Error> {
        // 1. Check memory cache first
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            tracing::debug!("Memory cache hit for version manifest: {}@{}", name, spec);
            return Ok(manifest);
        }

        // 2. Check disk cache (only for non-semver registries where spec is exact version)
        if !self.supports_semver
            && let Some(manifest) = self.cache.get_version_manifest_from_disk(name, spec).await
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
        let manifest = manifest::fetch_version_manifest(manifest::FetchVersionManifestOptions {
            registry_url: &self.registry_url,
            name,
            spec,
            format: if self.supports_semver {
                manifest::MetadataFormat::Abbreviated
            } else {
                manifest::MetadataFormat::Complete
            },
        })
        .await
        .map_err(RegistryError)?;

        let manifest = Arc::new(manifest);

        // 4. Cache the result
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest.clone());

        // 5. Write to disk cache (only for non-semver registries)
        if !self.supports_semver {
            self.cache
                .set_version_manifest_to_disk(name, spec, manifest.clone());
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
            // Borrow `keys` directly — `resolve_target_version` only needs
            // `&[String]`. The previous `keys.clone()` rebuilt a 100-500
            // entry `Vec<String>` per cache hit (≈1800 hits during a cold
            // ant-design preload), bloating per-future allocator pressure
            // by ~360k String allocs on shared resolver threads.
            let resolved_version = resolve_target_version(
                &full_manifest.dist_tags,
                &full_manifest.versions.keys,
                &fetch_spec,
            )
            .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
            let version_manifest = full_manifest
                .get_core_version(&resolved_version)
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
                self.cache.set_version_manifest_to_disk(
                    &fetch_name,
                    &resolved_version,
                    version_manifest.clone(),
                );
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
            let resolve_result = match self.resolve_full_manifest(&fetch_name).await? {
                FullManifestResult::Full(full_manifest) => {
                    // Got full manifest, resolve from it
                    let version_list: Vec<String> = full_manifest.versions.keys.clone();

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
                        .get_core_version(&resolved_version)
                        .ok_or_else(|| {
                            RegistryError(anyhow!(
                                "Version {} not found in manifest for {}",
                                resolved_version,
                                fetch_name
                            ))
                        })?;

                    (resolved_version, version_manifest)
                }
                FullManifestResult::NotModified => {
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
            };

            let (resolved_version, version_manifest) = resolve_result;

            // Cache in memory for project cache export
            self.cache.set_version_manifest(
                fetch_name.to_string(),
                fetch_spec.to_string(),
                version_manifest.clone(),
            );

            // Write to disk cache (only for non-semver registries)
            if !self.supports_semver {
                self.cache.set_version_manifest_to_disk(
                    &fetch_name,
                    &resolved_version,
                    version_manifest.clone(),
                );
            }

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
        let registry = UnifiedRegistry::builder().build();
        assert!(registry.supports_semver());
        assert_eq!(registry.registry_url(), "https://registry.npmmirror.com");

        // Custom registry
        let registry = UnifiedRegistry::builder()
            .registry("https://registry.npmjs.org")
            .build();
        assert!(!registry.supports_semver());
        assert_eq!(registry.registry_url(), "https://registry.npmjs.org");
    }

    #[test]
    fn test_unified_registry_builder_explicit_supports_semver() {
        // Explicitly override supports_semver for npm registry
        let registry = UnifiedRegistry::builder()
            .registry("https://registry.npmjs.org")
            .supports_semver(true)
            .build();
        assert!(registry.supports_semver());

        // Explicitly override supports_semver for non-npm registry
        let registry = UnifiedRegistry::builder()
            .registry("https://registry.npmmirror.com")
            .supports_semver(false)
            .build();
        assert!(!registry.supports_semver());

        // Without explicit override, auto-detect based on URL
        let registry = UnifiedRegistry::builder()
            .registry("https://registry.npmjs.org")
            .build();
        assert!(!registry.supports_semver());

        let registry = UnifiedRegistry::builder()
            .registry("https://registry.npmmirror.com")
            .build();
        assert!(registry.supports_semver());
    }

    #[test]
    fn test_unified_registry_with_cache_dir() {
        let registry = UnifiedRegistry::builder()
            .registry("https://registry.npmmirror.com")
            .cache_dir(PathBuf::from("/tmp/cache"))
            .build();
        assert!(registry.supports_semver());
        assert!(registry.cache().cache_dir().is_some());
    }

    #[test]
    fn test_unified_registry_with_shared_cache() {
        let shared_cache = Arc::new(PackageCache::with_cache_dir(PathBuf::from("/tmp/shared")));

        let registry1 = UnifiedRegistry::builder()
            .registry("https://registry.npmmirror.com")
            .cache(Arc::clone(&shared_cache))
            .build();

        let registry2 = UnifiedRegistry::builder()
            .registry("https://registry.npmmirror.com")
            .cache(Arc::clone(&shared_cache))
            .build();

        // Both registries share the same cache
        assert!(Arc::ptr_eq(&registry1.cache, &registry2.cache));
    }
}
