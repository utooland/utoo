//! Unified registry client implementation.
//!
//! Provides `UnifiedRegistry` that works on both native and WASM targets.
//! Combines HTTP fetching with in-memory caching, optional persistent storage
//! through a [`ManifestStore`], and automatic registry capability detection
//! (semver support).
//!
//! For non-semver registries (npmjs.org), the persistent store doubles as the
//! ETag source: `versions.json` carries the etag for the next conditional
//! GET, and per-version manifests act as a warm cache for `(name, spec)`
//! pairs.
//!
//! # Architecture
//!
//! - `manifest` module: Manifest fetching with retry (`fetch_full_manifest`, `fetch_version_manifest`)
//! - `UnifiedRegistry`: in-memory cache + injected `ManifestStore` (host-supplied persistence)
//!   - Memory cache (fastest)
//!   - `ManifestStore` (host: disk / KV / no-op)
//!   - Network (authoritative source)

use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;

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

use dashmap::DashSet;

use super::cache::{PackageCache, Versions, VersionsInfo};
use super::manifest;
use super::provider::{
    ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider, ProviderFullManifestBytes,
};
use super::store::{ManifestStore, NoopStore};
use crate::model::manifest::{CoreVersionManifest, FullManifest, extract_core_version_off_runtime};
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::traits::registry::{RegistryClient, RegistryError, ResolvedPackage, is_npm_registry};

/// Unified registry client that works on both native and WASM.
///
/// Cache lookup order:
/// 1. In-memory `PackageCache` (fastest, lost on restart)
/// 2. Host-provided `ManifestStore` (persistent; disk on native, no-op on WASM by default)
/// 3. Network (slowest, always authoritative)
///
/// For non-semver registries (npmjs.org), uses ETag validation to avoid
/// re-downloading unchanged manifests; the etag is sourced from
/// `ManifestStore::load_versions`.
///
/// # Example
///
/// ```ignore
/// // Using builder pattern
/// let registry = UnifiedRegistry::builder()
///     .registry("https://registry.npmmirror.com")
///     .store(Arc::new(MyManifestStore::new()))
///     .build();
/// ```
pub struct UnifiedRegistry {
    registry_url: String,
    cache: Arc<PackageCache>,
    store: Arc<dyn ManifestStore>,
    supports_semver: bool,
    /// Dedup set for `store_version_manifest` disk writes keyed by
    /// `(name, resolved_version)`. The BFS loop may schedule sibling specs
    /// (e.g. `^1.0.0` and `^1.2.0`) that resolve to the same version; first
    /// insert wins and subsequent specs skip the redundant write.
    stored_version: Arc<DashSet<(String, String)>>,
}

/// Builder for `UnifiedRegistry`.
pub struct UnifiedRegistryBuilder {
    registry_url: Option<String>,
    cache: Option<Arc<PackageCache>>,
    store: Option<Arc<dyn ManifestStore>>,
    supports_semver: Option<bool>,
}

impl UnifiedRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry_url: None,
            cache: None,
            store: None,
            supports_semver: None,
        }
    }

    /// Set the registry URL.
    pub fn registry(mut self, url: impl Into<String>) -> Self {
        self.registry_url = Some(url.into());
        self
    }

    /// Set the persistence backend. Defaults to [`NoopStore`].
    pub fn store(mut self, store: Arc<dyn ManifestStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Set a shared in-memory cache instance.
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

        let cache = self
            .cache
            .unwrap_or_else(|| Arc::new(PackageCache::default()));
        let store = self.store.unwrap_or_else(|| Arc::new(NoopStore));

        UnifiedRegistry {
            registry_url,
            cache,
            store,
            supports_semver,
            stored_version: Arc::new(DashSet::new()),
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
            store: Arc::clone(&self.store),
            supports_semver: self.supports_semver,
            stored_version: Arc::clone(&self.stored_version),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ManifestProvider for UnifiedRegistry {
    async fn execute_manifest_job(&self, job: ManifestJob) -> Result<ManifestJobDone, Self::Error> {
        match job {
            ManifestJob::Full { name } => {
                if let Some(manifest) = self.cache.get_full_manifest(&name) {
                    return Ok(ManifestJobDone::Full {
                        name,
                        data: ManifestFullData::Full(manifest),
                    });
                }

                let data = match self.fetch_full_manifest_job(&name).await? {
                    ProviderFullManifestBytes::Fresh { bytes, etag } => {
                        let manifest =
                            Arc::new(manifest::parse_full_manifest_off_runtime(bytes).await?);
                        let versions = Arc::new(VersionsInfo {
                            versions: Versions {
                                version_list: manifest.versions.clone(),
                                dist_tags: manifest.dist_tags.clone(),
                            },
                            etag,
                            last_updated: current_timestamp_secs(),
                        });
                        self.cache
                            .set_full_manifest(name.clone(), Arc::clone(&manifest));
                        self.store.store_versions(&name, versions);
                        ManifestFullData::Full(manifest)
                    }
                    ProviderFullManifestBytes::NotModified { versions } => {
                        self.cache.set_versions(name.clone(), Arc::clone(&versions));
                        ManifestFullData::Versions(versions)
                    }
                };

                Ok(ManifestJobDone::Full { name, data })
            }
            ManifestJob::Version {
                name,
                spec,
                fetch_spec,
                format,
            } => {
                if let Some(manifest) = self.cache.get_version_manifest(&name, &spec) {
                    return Ok(ManifestJobDone::Version {
                        name,
                        spec,
                        manifest,
                    });
                }

                if deno_semver::Version::parse_from_npm(&fetch_spec).is_ok()
                    && let Some(manifest) =
                        self.store.load_version_manifest(&name, &fetch_spec).await
                {
                    let manifest = Arc::new(manifest);
                    self.cache_version_job_result(&name, &spec, Arc::clone(&manifest));
                    return Ok(ManifestJobDone::Version {
                        name,
                        spec,
                        manifest,
                    });
                }

                let bytes =
                    manifest::fetch_version_manifest_bytes(manifest::FetchVersionManifestOptions {
                        registry_url: &self.registry_url,
                        name: &name,
                        spec: &fetch_spec,
                        format,
                    })
                    .await
                    .map_err(RegistryError)?;
                let manifest =
                    Arc::new(manifest::parse_json_off_runtime::<CoreVersionManifest>(bytes).await?);
                self.cache_version_job_result(&name, &spec, Arc::clone(&manifest));
                Ok(ManifestJobDone::Version {
                    name,
                    spec,
                    manifest,
                })
            }
            ManifestJob::ExtractVersion {
                name,
                spec,
                version,
                full,
            } => {
                if let Some(manifest) = self.cache.get_version_manifest(&name, &version) {
                    self.cache_version_job_result(&name, &spec, Arc::clone(&manifest));
                    return Ok(ManifestJobDone::Version {
                        name,
                        spec,
                        manifest,
                    });
                }

                let (resolved_version, manifest) =
                    extract_core_version_off_runtime(full, version).await;
                let manifest = manifest.ok_or_else(|| {
                    RegistryError(anyhow!(
                        "Version {} not found in manifest for {}",
                        resolved_version,
                        name
                    ))
                })?;
                self.cache_version_job_result(&name, &spec, Arc::clone(&manifest));
                Ok(ManifestJobDone::Version {
                    name,
                    spec,
                    manifest,
                })
            }
        }
    }
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

    /// Get the underlying in-memory cache.
    pub fn cache(&self) -> &PackageCache {
        &self.cache
    }

    fn cache_version_job_result(&self, name: &str, spec: &str, manifest: Arc<CoreVersionManifest>) {
        self.cache
            .set_version_manifest(name.to_string(), spec.to_string(), Arc::clone(&manifest));
        if manifest.version != spec {
            self.cache.set_version_manifest(
                name.to_string(),
                manifest.version.clone(),
                Arc::clone(&manifest),
            );
        }
        if self
            .stored_version
            .insert((name.to_string(), manifest.version.clone()))
        {
            self.store
                .store_version_manifest(name, &manifest.version, Arc::clone(&manifest));
        }
    }

    fn version_metadata_format(&self) -> manifest::MetadataFormat {
        if self.supports_semver {
            manifest::MetadataFormat::Abbreviated
        } else {
            manifest::MetadataFormat::Complete
        }
    }

    async fn fetch_full_manifest_job(
        &self,
        name: &str,
    ) -> Result<ProviderFullManifestBytes, RegistryError> {
        let store_versions = self.store.load_versions(name).await.map(Arc::new);
        let etag = store_versions.as_ref().and_then(|v| v.etag.clone());

        match manifest::fetch_full_manifest_bytes(manifest::FetchManifestOptions {
            registry_url: &self.registry_url,
            name,
            format: manifest::MetadataFormat::Abbreviated,
            etag: etag.as_deref(),
        })
        .await
        .map_err(RegistryError)?
        {
            manifest::FetchManifestBytesResult::Ok(bytes, etag) => {
                Ok(ProviderFullManifestBytes::Fresh { bytes, etag })
            }
            manifest::FetchManifestBytesResult::NotModified => {
                let versions = store_versions.ok_or_else(|| {
                    RegistryError(anyhow!(
                        "304 Not Modified without cached versions for {name}"
                    ))
                })?;
                Ok(ProviderFullManifestBytes::NotModified { versions })
            }
        }
    }

    async fn execute_version_job(
        &self,
        name: &str,
        spec: &str,
        fetch_spec: &str,
    ) -> Result<Arc<CoreVersionManifest>, RegistryError> {
        match self
            .execute_manifest_job(ManifestJob::Version {
                name: name.to_string(),
                spec: spec.to_string(),
                fetch_spec: fetch_spec.to_string(),
                format: self.version_metadata_format(),
            })
            .await?
        {
            ManifestJobDone::Version { manifest, .. } => Ok(manifest),
            ManifestJobDone::Full { .. } => Err(RegistryError(anyhow!(
                "provider returned full manifest for version job {name}@{spec}"
            ))),
        }
    }

    async fn execute_extract_job(
        &self,
        name: &str,
        spec: &str,
        version: String,
        full: Arc<FullManifest>,
    ) -> Result<Arc<CoreVersionManifest>, RegistryError> {
        match self
            .execute_manifest_job(ManifestJob::ExtractVersion {
                name: name.to_string(),
                spec: spec.to_string(),
                version,
                full,
            })
            .await?
        {
            ManifestJobDone::Version { manifest, .. } => Ok(manifest),
            ManifestJobDone::Full { .. } => Err(RegistryError(anyhow!(
                "provider returned full manifest for extract job {name}@{spec}"
            ))),
        }
    }

    /// Compatibility wrapper for direct `RegistryClient` callers. The normal
    /// install/deps path resolves in the BFS loop; this path executes the same
    /// provider jobs without adding a second inflight layer.
    async fn resolve_version_manifest_job(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<Arc<CoreVersionManifest>, RegistryError> {
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            return Ok(manifest);
        }

        if self.supports_semver {
            return self.execute_version_job(name, spec, spec).await;
        }

        match self
            .execute_manifest_job(ManifestJob::Full {
                name: name.to_string(),
            })
            .await?
        {
            ManifestJobDone::Full {
                data: ManifestFullData::Full(full),
                ..
            } => {
                if full.versions.is_empty() {
                    return Err(RegistryError(anyhow!("No versions available for {}", name)));
                }
                let resolved_version = resolve_target_version((&*full).into(), spec)
                    .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
                if let Some(cached) = self.cache.get_version_manifest(name, &resolved_version) {
                    self.cache_version_job_result(name, spec, Arc::clone(&cached));
                    return Ok(cached);
                }
                self.execute_extract_job(name, spec, resolved_version, full)
                    .await
            }
            ManifestJobDone::Full {
                data: ManifestFullData::Versions(versions),
                ..
            } => {
                if versions.versions.version_list.is_empty() {
                    return Err(RegistryError(anyhow!("No versions available for {}", name)));
                }
                let resolved_version = resolve_target_version((&*versions).into(), spec)
                    .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
                self.execute_version_job(name, spec, &resolved_version)
                    .await
            }
            ManifestJobDone::Version { .. } => Err(RegistryError(anyhow!(
                "provider returned version manifest for full job {name}"
            ))),
        }
    }
}

impl RegistryClient for UnifiedRegistry {
    type Error = RegistryError;

    fn supports_semver_resolution(&self) -> bool {
        self.supports_semver
    }

    fn registry_url(&self) -> &str {
        &self.registry_url
    }

    async fn fetch_full_manifest(&self, name: &str) -> Result<Arc<FullManifest>, Self::Error> {
        match self
            .execute_manifest_job(ManifestJob::Full {
                name: name.to_string(),
            })
            .await?
        {
            ManifestJobDone::Full {
                data: ManifestFullData::Full(manifest),
                ..
            } => Ok(manifest),
            ManifestJobDone::Full {
                data: ManifestFullData::Versions(_),
                ..
            } => Err(RegistryError(anyhow!(
                "No full manifest available for {} (304 Not Modified)",
                name
            ))),
            ManifestJobDone::Version { .. } => Err(RegistryError(anyhow!(
                "provider returned version manifest for full job {name}"
            ))),
        }
    }

    async fn fetch_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<Arc<CoreVersionManifest>, Self::Error> {
        self.resolve_version_manifest_job(name, spec).await
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

        let manifest = self
            .resolve_version_manifest_job(&fetch_name, &fetch_spec)
            .await?;
        Ok(ResolvedPackage {
            name: name.to_string(),
            version: manifest.version.clone(),
            manifest,
        })
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
    fn test_unified_registry_with_shared_cache() {
        let shared_cache = Arc::new(PackageCache::default());

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
