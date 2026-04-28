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
use crate::util::OnceMap;

/// Inflight outcome for a full-manifest fetch — discriminates the cache
/// state without re-cloning the manifest into the OnceMap.
#[derive(Clone, Copy)]
enum InflightFull {
    /// 200: full manifest now in `PackageCache::set_full_manifest` — re-read it.
    Full,
    /// 304: only `VersionsInfo` was refreshed; caller must surface as
    /// `FullManifestResult::NotModified`.
    NotModified,
}

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
    /// Single-flight gate for full-manifest fetches keyed by package name.
    /// Concurrent resolves for the same name share the underlying network +
    /// disk work; result is read back from `cache` (or surfaced as 304).
    inflight_full: Arc<OnceMap<String, InflightFull>>,
    /// Single-flight gate for version-manifest fetches keyed by `(name, spec)`.
    /// `name@spec` is used because semver registries resolve spec server-side,
    /// so two requests with the same name but different specs must not share.
    inflight_version: Arc<OnceMap<(String, String), CoreVersionManifest>>,
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
            inflight_full: Arc::new(OnceMap::new()),
            inflight_version: Arc::new(OnceMap::new()),
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
            inflight_full: Arc::clone(&self.inflight_full),
            inflight_version: Arc::clone(&self.inflight_version),
        }
    }
}

/// Result of `resolve_full_manifest`.
///
/// Separates the 200 (full data) and 304 (use cache) cases at the type level,
/// so callers can pattern-match instead of string-matching error messages.
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
        // 1. Check memory cache first (fast path before contending on the inflight map).
        if let Some(manifest) = self.cache.get_full_manifest(name) {
            tracing::debug!("Memory cache hit for full manifest: {}", name);
            return Ok(FullManifestResult::Full(manifest));
        }

        // 2. Single-flight: dedup concurrent disk + network work for the same
        //    package name. Init populates `cache` as a side effect; both worker
        //    and waiters then read the populated cache below.
        let outcome = self
            .inflight_full
            .get_or_try_init::<RegistryError, _, _>(name.to_string(), || async {
                // Re-check memory cache inside the worker — a previous flight may
                // have populated it while we were queuing on dashmap's shard lock.
                if self.cache.get_full_manifest(name).is_some() {
                    return Ok(InflightFull::Full);
                }

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
                        let versions_info = Arc::new(VersionsInfo {
                            versions: Versions {
                                version_list: manifest.versions.clone(),
                                dist_tags: manifest.dist_tags.clone(),
                            },
                            etag: new_etag,
                            last_updated: current_timestamp_secs(),
                        });
                        self.cache
                            .set_full_manifest(name.to_string(), Arc::new(manifest));
                        self.cache
                            .set_versions(name.to_string(), versions_info.clone());
                        self.cache.set_versions_to_disk(name, &versions_info).await;
                        Ok(InflightFull::Full)
                    }
                    manifest::FetchManifestResult::NotModified => {
                        tracing::debug!("ETag cache hit (304) for: {}", name);
                        if let Some(versions_info) = disk_versions {
                            self.cache.set_versions(name.to_string(), versions_info);
                            Ok(InflightFull::NotModified)
                        } else {
                            // Disk cache corrupted, fetch fresh (without etag)
                            let (manifest, new_etag) = manifest::fetch_full_manifest_fresh(
                                &self.registry_url,
                                name,
                                manifest::MetadataFormat::Abbreviated,
                            )
                            .await
                            .map_err(RegistryError)?;

                            let versions_info = Arc::new(VersionsInfo {
                                versions: Versions {
                                    version_list: manifest.versions.clone(),
                                    dist_tags: manifest.dist_tags.clone(),
                                },
                                etag: new_etag,
                                last_updated: current_timestamp_secs(),
                            });
                            self.cache
                                .set_full_manifest(name.to_string(), Arc::new(manifest));
                            self.cache
                                .set_versions(name.to_string(), versions_info.clone());
                            self.cache.set_versions_to_disk(name, &versions_info).await;
                            Ok(InflightFull::Full)
                        }
                    }
                }
            })
            .await?;

        match *outcome {
            InflightFull::Full => {
                // Populated by the inflight worker; missing here would only mean
                // a race with cache eviction — surface a clear error.
                self.cache
                    .get_full_manifest(name)
                    .map(FullManifestResult::Full)
                    .ok_or_else(|| {
                        RegistryError(anyhow!(
                            "full manifest for {name} vanished from cache after fetch"
                        ))
                    })
            }
            InflightFull::NotModified => Ok(FullManifestResult::NotModified),
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
        // 1. Check memory cache using name@spec as key (fast path).
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            tracing::debug!("Memory cache hit for version manifest: {}@{}", name, spec);
            return Ok(manifest);
        }

        // 2. Single-flight: concurrent resolves for the same (name, spec) share
        //    the disk-cache check + network fetch. The OnceMap-stored
        //    `Arc<CoreVersionManifest>` is the canonical result; PackageCache
        //    is populated as a side effect for cross-registry/cross-instance
        //    sharing via the global memory cache singleton.
        self.inflight_version
            .get_or_try_init::<RegistryError, _, _>(
                (name.to_string(), spec.to_string()),
                || async {
                    // Re-check memory cache inside the worker (covers the brief
                    // window between fast-path miss and shard-lock acquire).
                    if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
                        // OnceMap requires owning the V; clone the inner manifest
                        // out of its Arc.
                        return Ok((*manifest).clone());
                    }

                    if !self.supports_semver
                        && let Some(manifest) =
                            self.cache.get_version_manifest_from_disk(name, spec).await
                    {
                        tracing::debug!("Disk cache hit for version manifest: {}@{}", name, spec);
                        return Ok((*manifest).clone());
                    }

                    tracing::debug!("Cache miss for {}@{}, fetching from network", name, spec);
                    let manifest =
                        manifest::fetch_version_manifest(manifest::FetchVersionManifestOptions {
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

                    let arc_manifest = Arc::new(manifest.clone());
                    self.cache.set_version_manifest(
                        name.to_string(),
                        spec.to_string(),
                        arc_manifest.clone(),
                    );
                    if !self.supports_semver {
                        self.cache
                            .set_version_manifest_to_disk(name, spec, &arc_manifest)
                            .await;
                    }
                    Ok(manifest)
                },
            )
            .await
    }
}

impl RegistryClient for UnifiedRegistry {
    type Error = RegistryError;

    fn supports_semver_resolution(&self) -> bool {
        self.supports_semver
    }

    fn cache_version_manifest(&self, name: &str, spec: &str, manifest: Arc<CoreVersionManifest>) {
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), manifest);
    }

    async fn fetch_full_manifest(&self, name: &str) -> Result<Arc<FullManifest>, Self::Error> {
        match self.resolve_full_manifest(name).await? {
            FullManifestResult::Full(manifest) => Ok(manifest),
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
        // Delegates to `resolve_version_manifest` so the inflight dedup +
        // memory/disk cache logic lives in one place.
        self.resolve_version_manifest(name, spec).await
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
            let version_list: Vec<String> = full_manifest.versions.clone();
            let resolved_version =
                resolve_target_version(&full_manifest.dist_tags, &version_list, &fetch_spec)
                    .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
            let version_manifest = full_manifest
                .get_core_version(&resolved_version)
                .map(Arc::new)
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
                    .set_version_manifest_to_disk(&fetch_name, &resolved_version, &version_manifest)
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
            let resolve_result = match self.resolve_full_manifest(&fetch_name).await? {
                FullManifestResult::Full(full_manifest) => {
                    // Got full manifest, resolve from it
                    let version_list: Vec<String> = full_manifest.versions.clone();

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
                        .map(Arc::new)
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
                self.cache
                    .set_version_manifest_to_disk(&fetch_name, &resolved_version, &version_manifest)
                    .await;
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
