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
use super::store::{ManifestStore, NoopStore};
use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::traits::registry::{RegistryClient, RegistryError, ResolvedPackage, is_npm_registry};
use crate::util::OnceMap;

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
    /// Single-flight gate for full-manifest fetches keyed by package name.
    /// **Gate-only**: the entry value is `()`; the canonical
    /// `Arc<FullManifest>` lives in `PackageCache`. Concurrent resolves for
    /// the same name share one network + parse round-trip; the
    /// 200/304 outcome is recovered by inspecting cache state after the
    /// gate releases.
    inflight_full: Arc<OnceMap<String, ()>>,
    /// Single-flight gate for version-manifest fetches keyed by
    /// `(name, spec)`. Same gate-only pattern: the canonical `Arc<…>`
    /// lives in `PackageCache.version_manifests`; the gate stores `()`.
    inflight_version: Arc<OnceMap<(String, String), ()>>,
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

        let cache = self.cache.unwrap_or_else(|| Arc::new(PackageCache::new()));
        let store = self.store.unwrap_or_else(|| Arc::new(NoopStore));

        UnifiedRegistry {
            registry_url,
            cache,
            store,
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
            store: Arc::clone(&self.store),
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

    /// Get the underlying in-memory cache.
    pub fn cache(&self) -> &PackageCache {
        &self.cache
    }

    /// Resolve full manifest through memory → store → network with ETag validation.
    ///
    /// Single-flight cache flow:
    /// 1. Memory cache hit on `full_manifests` → return immediately (Arc bump).
    /// 2. Otherwise, acquire the gate-only `inflight_full<name>`. The worker
    ///    closure populates `PackageCache` as a side effect: writes
    ///    `full_manifests` on 200, writes only `versions_info` on 304.
    /// 3. After the gate releases, recover the outcome by inspecting cache
    ///    state — `full_manifests` populated → 200, only `versions_info`
    ///    populated → 304.
    async fn resolve_full_manifest(&self, name: &str) -> Result<FullManifestResult, RegistryError> {
        if let Some(manifest) = self.cache.get_full_manifest(name) {
            return Ok(FullManifestResult::Full(manifest));
        }

        self.inflight_full
            .get_or_try_init::<RegistryError, _, _>(name.to_string(), || async {
                // Re-check inside the worker — a previous winner may have
                // populated the cache while we queued on the OnceMap shard.
                if self.cache.get_full_manifest(name).is_some() {
                    return Ok(());
                }

                let store_versions = self.store.load_versions(name).await.map(Arc::new);
                let etag = store_versions.as_ref().and_then(|v| v.etag.clone());

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
                            .set_versions(name.to_string(), Arc::clone(&versions_info));
                        // Fire-and-forget: store may spawn its own task.
                        self.store.store_versions(name, versions_info);
                    }
                    manifest::FetchManifestResult::NotModified => {
                        tracing::debug!("ETag cache hit (304) for: {}", name);
                        if let Some(versions_info) = store_versions {
                            // Only populate `versions_info`; absence of
                            // `full_manifests` after the gate is the 304
                            // signal.
                            self.cache.set_versions(name.to_string(), versions_info);
                        } else {
                            // Persistent store corrupted/missing, fetch fresh (without etag).
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
                                .set_versions(name.to_string(), Arc::clone(&versions_info));
                            self.store.store_versions(name, versions_info);
                        }
                    }
                }
                Ok(())
            })
            .await?;

        // Cache state is the discriminator: `full_manifests` populated → 200;
        // only `versions_info` populated → 304; neither → cache eviction race.
        if let Some(manifest) = self.cache.get_full_manifest(name) {
            Ok(FullManifestResult::Full(manifest))
        } else if self.cache.get_versions(name).is_some() {
            Ok(FullManifestResult::NotModified)
        } else {
            Err(RegistryError(anyhow!(
                "manifest for {name} vanished from cache after fetch"
            )))
        }
    }

    /// Resolve version manifest through memory → store → network.
    ///
    /// Cache key is `name@spec` (e.g., `lodash@^4.17.0`), so the same spec
    /// requested multiple times shares one fetch.
    ///
    /// Non-semver registries resolve the spec by extracting the matching
    /// version from the full manifest (the latter is itself single-flight
    /// gated by `inflight_full`). Semver registries query the version
    /// manifest directly. Either way the work for one `(name, spec)` runs
    /// once; concurrent callers for the same key share the result.
    async fn resolve_version_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<Arc<CoreVersionManifest>, RegistryError> {
        if let Some(manifest) = self.cache.get_version_manifest(name, spec) {
            return Ok(manifest);
        }

        self.inflight_version
            .get_or_try_init::<RegistryError, _, _>(
                (name.to_string(), spec.to_string()),
                || async {
                    // Re-check inside the worker (covers the brief window
                    // between fast-path miss and OnceMap shard-lock acquire).
                    if self.cache.get_version_manifest(name, spec).is_some() {
                        return Ok(());
                    }

                    if !self.supports_semver
                        && let Some(manifest) = self.store.load_version_manifest(name, spec).await
                    {
                        tracing::debug!("Persistent store hit for version manifest: {name}@{spec}");
                        // Populate memory cache ourselves — store knows nothing about it.
                        self.cache.set_version_manifest(
                            name.to_string(),
                            spec.to_string(),
                            Arc::new(manifest),
                        );
                        return Ok(());
                    }

                    if !self.supports_semver {
                        // Non-semver: resolve the spec by extracting the matching
                        // version from the full manifest. `resolve_full_manifest`
                        // is itself inflight-gated, so concurrent specs for the
                        // same name share one full-manifest fetch.
                        let (resolved_version, manifest) =
                            self.resolve_via_full_manifest(name, spec).await?;
                        let arc = Arc::new(manifest);
                        self.cache.set_version_manifest(
                            name.to_string(),
                            spec.to_string(),
                            Arc::clone(&arc),
                        );
                        if resolved_version != spec {
                            self.cache.set_version_manifest(
                                name.to_string(),
                                resolved_version.clone(),
                                Arc::clone(&arc),
                            );
                        }
                        self.store.store_version_manifest(
                            name,
                            &resolved_version,
                            Arc::clone(&arc),
                        );
                        return Ok(());
                    }

                    tracing::debug!("Cache miss for {}@{}, fetching from network", name, spec);
                    let manifest =
                        manifest::fetch_version_manifest(manifest::FetchVersionManifestOptions {
                            registry_url: &self.registry_url,
                            name,
                            spec,
                            format: manifest::MetadataFormat::Abbreviated,
                        })
                        .await
                        .map_err(RegistryError)?;

                    self.cache.set_version_manifest(
                        name.to_string(),
                        spec.to_string(),
                        Arc::new(manifest),
                    );
                    Ok(())
                },
            )
            .await?;

        // Gate released — populated either by us, a prior waiter, or a previous
        // run that hit memory/disk cache. Missing only on cache eviction race.
        self.cache.get_version_manifest(name, spec).ok_or_else(|| {
            RegistryError(anyhow!(
                "version manifest for {name}@{spec} vanished from cache after fetch"
            ))
        })
    }

    /// Resolve `(name, spec)` for non-semver registries by reading the full
    /// manifest and extracting the matching version.
    ///
    /// Handles the 304 (NotModified) case by falling back to the in-memory
    /// versions cache for resolution and a single-version network fetch for
    /// the manifest itself. The caller is responsible for caching the
    /// extracted manifest; this helper does not touch `PackageCache`.
    async fn resolve_via_full_manifest(
        &self,
        name: &str,
        spec: &str,
    ) -> Result<(String, CoreVersionManifest), RegistryError> {
        match self.resolve_full_manifest(name).await? {
            FullManifestResult::Full(full) => {
                if full.versions.is_empty() {
                    return Err(RegistryError(anyhow!("No versions available for {}", name)));
                }
                let resolved_version =
                    resolve_target_version(&full.dist_tags, &full.versions, spec)
                        .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
                let core = full.get_core_version(&resolved_version).ok_or_else(|| {
                    RegistryError(anyhow!(
                        "Version {} not found in manifest for {}",
                        resolved_version,
                        name
                    ))
                })?;
                Ok((resolved_version, core))
            }
            FullManifestResult::NotModified => {
                // 304 fallback: ETag matched, full payload not refetched.
                // Resolve via the lightweight versions cache, then hit the
                // network for the single requested version. Direct call into
                // `manifest::fetch_version_manifest` (not `self.resolve_version_manifest`)
                // to avoid re-entering the inflight_version gate; the outer
                // `inflight_version<(name, spec)>` already serializes us.
                let versions_info = self.cache.get_versions(name).ok_or_else(|| {
                    RegistryError(anyhow!("Versions cache not found for {}", name))
                })?;
                let resolved_version = resolve_target_version(
                    &versions_info.versions.dist_tags,
                    &versions_info.versions.version_list,
                    spec,
                )
                .map_err(|e| RegistryError(anyhow!("{}@{}: {}", name, spec, e)))?;
                let manifest =
                    manifest::fetch_version_manifest(manifest::FetchVersionManifestOptions {
                        registry_url: &self.registry_url,
                        name,
                        spec: &resolved_version,
                        format: manifest::MetadataFormat::Complete,
                    })
                    .await
                    .map_err(RegistryError)?;
                Ok((resolved_version, manifest))
            }
        }
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
        // memory/store cache logic lives in one place.
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

        // Single entry point: `resolve_version_manifest` covers both semver
        // (direct version-manifest fetch) and non-semver (full-manifest +
        // extract) paths, with `inflight_version<(name, spec)>` ensuring one
        // fetch+extraction per `(name, spec)` regardless of registry type.
        let manifest = self
            .resolve_version_manifest(&fetch_name, &fetch_spec)
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
        let shared_cache = Arc::new(PackageCache::new());

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
