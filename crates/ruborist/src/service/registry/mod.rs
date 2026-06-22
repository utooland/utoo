//! Unified registry client implementation.
//!
//! Provides `UnifiedRegistry`, a stateless [`ManifestProvider`] that works on
//! both native and WASM targets. It owns no resolution state — version
//! selection and de-duplication live in the demand resolver (see
//! [`crate::resolver::demand`]); the registry only fetches and parses one
//! manifest job at a time (see the child [`provider`] module) and persists
//! through the injected [`ManifestStore`].
//!
//! For non-semver registries (npmjs.org), the persistent store doubles as the
//! ETag source: `versions.json` carries the etag for the next conditional
//! GET, and per-version manifests act as a warm cache for `(name, spec)`
//! pairs.

use std::sync::Arc;

use dashmap::DashSet;

use super::manifest;
use super::store::{ManifestStore, NoopStore};
use crate::traits::registry::{RegistryClient, RegistryError, is_npm_registry};

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

/// `ManifestProvider` implementation for the demand BFS resolver. A child
/// module so it can reach `UnifiedRegistry`'s private fields without widening
/// their visibility.
mod provider;

/// Stateless registry client that works on both native and WASM.
///
/// Resolution order per fetch: host-provided `ManifestStore` (persistent;
/// disk on native, no-op on WASM by default) → network. For non-semver
/// registries (npmjs.org), uses ETag validation to avoid re-downloading
/// unchanged manifests; the etag is sourced from `ManifestStore::load_versions`.
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
    /// Bearer token for a private registry, attached to manifest requests
    /// against `registry_url`. `None` for public registries.
    auth_token: Option<String>,
    store: Arc<dyn ManifestStore>,
    supports_semver: bool,
    /// Dedup set for `store_version_manifest` disk writes keyed by
    /// `(name, resolved_version)`. Sibling specs (e.g. `^1.0.0` and `^1.2.0`)
    /// resolving to the same version would otherwise issue duplicate writes
    /// for the same path. First insert wins; later specs skip the redundant
    /// write.
    stored_version: Arc<DashSet<(String, String)>>,
}

/// Builder for `UnifiedRegistry`.
pub struct UnifiedRegistryBuilder {
    registry_url: Option<String>,
    auth_token: Option<String>,
    store: Option<Arc<dyn ManifestStore>>,
    supports_semver: Option<bool>,
}

impl UnifiedRegistryBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry_url: None,
            auth_token: None,
            store: None,
            supports_semver: None,
        }
    }

    /// Set the registry URL.
    pub fn registry(mut self, url: impl Into<String>) -> Self {
        self.registry_url = Some(url.into());
        self
    }

    /// Set the Bearer token for a private registry. `None` leaves requests
    /// unauthenticated. ruborist never resolves the token itself — the host
    /// (pm) reads it from env/config and injects it here.
    pub fn auth_token(mut self, token: Option<String>) -> Self {
        self.auth_token = token;
        self
    }

    /// Set the persistence backend. Defaults to [`NoopStore`].
    pub fn store(mut self, store: Arc<dyn ManifestStore>) -> Self {
        self.store = Some(store);
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

        let store = self.store.unwrap_or_else(|| Arc::new(NoopStore));

        UnifiedRegistry {
            registry_url,
            auth_token: self.auth_token,
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
            auth_token: self.auth_token.clone(),
            store: Arc::clone(&self.store),
            supports_semver: self.supports_semver,
            stored_version: Arc::clone(&self.stored_version),
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

    /// Fetch the raw version-manifest JSON for `name@spec` through this
    /// registry — URL construction and auth are applied here, not by the
    /// caller. Returns the complete document so custom fields the typed model
    /// omits are preserved (e.g. `binary-mirror-config`'s `mirrors`).
    pub async fn fetch_version_manifest_bytes(
        &self,
        name: &str,
        spec: &str,
    ) -> anyhow::Result<Vec<u8>> {
        manifest::fetch_version_manifest_vec(manifest::FetchVersionManifestOptions {
            registry_url: &self.registry_url,
            name,
            spec,
            format: manifest::MetadataFormat::Complete,
            auth_token: self.auth_token.as_deref(),
        })
        .await
    }
}

impl RegistryClient for UnifiedRegistry {
    type Error = RegistryError;

    fn supports_semver_resolution(&self) -> bool {
        self.supports_semver
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
}
