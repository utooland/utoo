//! Registry client trait for dependency resolution.

use std::collections::HashMap;
use std::sync::Arc;

use crate::model::manifest::CoreVersionManifest;
#[cfg(test)]
use crate::model::manifest::FullManifest;
#[cfg(test)]
use crate::resolver::version::resolve_target_version;

/// Check if a registry URL is the official npm registry.
///
/// The official npm registry (registry.npmjs.org/com) does not support:
/// - Abbreviated manifests (application/vnd.npm.install-v1+json)
/// - Direct semver queries (registry/package/^1.0.0)
///
/// Mirror registries like npmmirror support these features for better performance.
///
/// # Example
/// ```
/// use utoo_ruborist::registry::is_npm_registry;
///
/// assert!(is_npm_registry("https://registry.npmjs.org"));
/// assert!(is_npm_registry("https://registry.npmjs.com"));
/// assert!(!is_npm_registry("https://registry.npmmirror.com"));
/// ```
pub fn is_npm_registry(url: &str) -> bool {
    url.contains("registry.npmjs.org") || url.contains("registry.npmjs.com")
}

/// Generic error wrapper for RegistryClient implementations.
///
/// Wraps anyhow::Error to implement std::error::Error trait
/// required by RegistryClient.
#[derive(Debug)]
pub struct RegistryError(pub anyhow::Error);

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl From<anyhow::Error> for RegistryError {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}

/// Resolved package information from registry.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    /// Package name
    pub name: String,
    /// Resolved version
    pub version: String,
    /// Slim package manifest for resolution/install (Arc-shared)
    pub manifest: Arc<CoreVersionManifest>,
}

/// Versions info for a package (lightweight, without full manifests).
#[derive(Debug, Clone)]
pub struct VersionsInfo {
    /// List of all available versions
    pub version_list: Vec<String>,
    /// Dist-tags (e.g., {"latest": "1.2.3", "beta": "2.0.0-beta.1"})
    pub dist_tags: HashMap<String, String>,
}

/// Registry client trait for fetching package information.
///
/// This trait handles package metadata fetching from the registry with support
/// for both traditional (full manifest) and semver-supporting registries.
///
/// # Registry Types
///
/// 1. **Traditional registries (npm)**: Only support fetching full package manifests
///    - Use `fetch_full_manifest` to get all versions
///    - Version resolution is done client-side
///
/// 2. **Semver-supporting registries (npmmirror, etc.)**: Support direct version queries
///    - Can fetch specific version manifest via `registry/package/^1.0.0`
///    - More efficient as no client-side version resolution needed
///
/// # Implementation Guide
///
/// - Override `supports_semver_resolution()` to return `true` if your registry supports it
/// - Override `fetch_version_manifest()` if your registry supports semver queries
/// - The `resolve_package()` method automatically chooses the best strategy
///
/// # Example Implementation
/// ```ignore
/// struct SemverSupportingRegistry;
///
/// impl RegistryClient for SemverSupportingRegistry {
///     type Error = MyError;
///
///     fn supports_semver_resolution(&self) -> bool {
///         true // This registry supports semver queries
///     }
///
///     async fn fetch_full_manifest(&self, name: &str) -> Result<FullManifest, Self::Error> {
///         // Fetch full manifest...
///     }
///
///     async fn fetch_version_manifest(&self, name: &str, spec: &str)
///         -> Result<VersionManifest, Self::Error>
///     {
///         // Fetch specific version via registry/name/spec
///     }
/// }
/// ```
pub trait RegistryClient {
    /// Error type for registry operations.
    ///
    /// Must implement `From<RegistryError>` to allow default implementations
    /// to convert internal errors to the client's error type.
    type Error: std::error::Error + From<RegistryError> + 'static;

    /// Whether this registry supports semver resolution (e.g., `registry/package/^1.0.0`).
    ///
    /// When true, `resolve_package` will use `fetch_version_manifest` directly.
    /// When false, `resolve_package` will fetch full manifest and resolve locally.
    ///
    /// Default: `false` (traditional npm registry behavior)
    fn supports_semver_resolution(&self) -> bool {
        false
    }
}

/// A simple in-memory registry client for testing.
#[cfg(test)]
pub mod mock {
    use super::*;

    /// Internal package data for mock registry.
    #[derive(Clone)]
    struct MockPackage {
        name: String,
        dist_tags: HashMap<String, String>,
        versions: HashMap<String, serde_json::Value>,
    }

    /// Mock registry client that returns predefined packages.
    #[derive(Clone)]
    pub struct MockRegistryClient {
        packages: HashMap<String, MockPackage>,
    }

    impl MockRegistryClient {
        pub fn new() -> Self {
            Self {
                packages: HashMap::new(),
            }
        }

        pub fn add_package(&mut self, name: &str, version: &str, manifest: CoreVersionManifest) {
            let pkg = self
                .packages
                .entry(name.to_string())
                .or_insert_with(|| MockPackage {
                    name: name.to_string(),
                    dist_tags: HashMap::new(),
                    versions: HashMap::new(),
                });

            pkg.versions.insert(
                version.to_string(),
                serde_json::to_value(&manifest).expect("CoreVersionManifest serialization"),
            );
            // Only set latest if not already set
            pkg.dist_tags
                .entry("latest".to_string())
                .or_insert_with(|| version.to_string());
        }

        pub fn set_latest(&mut self, name: &str, version: &str) {
            if let Some(pkg) = self.packages.get_mut(name) {
                pkg.dist_tags
                    .insert("latest".to_string(), version.to_string());
            }
        }
    }

    impl Default for MockRegistryClient {
        fn default() -> Self {
            Self::new()
        }
    }

    #[derive(Debug, Clone)]
    pub struct MockError(pub String);

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "MockError: {}", self.0)
        }
    }

    impl std::error::Error for MockError {}

    impl From<RegistryError> for MockError {
        fn from(e: RegistryError) -> Self {
            MockError(e.0.to_string())
        }
    }

    impl RegistryClient for MockRegistryClient {
        type Error = MockError;
    }

    impl MockRegistryClient {
        async fn fetch_full_manifest(&self, name: &str) -> Result<Arc<FullManifest>, MockError> {
            let pkg = self
                .packages
                .get(name)
                .ok_or_else(|| MockError(format!("Package not found: {}", name)))?;

            // Build JSON and serialize to raw bytes for on-demand extraction
            let json = serde_json::json!({
                "name": &pkg.name,
                "dist-tags": &pkg.dist_tags,
                "versions": &pkg.versions,
            });
            let raw = serde_json::to_vec(&json).expect("mock JSON serialization");

            Ok(Arc::new(FullManifest {
                name: pkg.name.clone(),
                dist_tags: pkg.dist_tags.clone(),
                versions: pkg.versions.keys().cloned().collect(),
                raw: bytes::Bytes::from(raw),
                ..Default::default()
            }))
        }

        async fn fetch_version_manifest(
            &self,
            name: &str,
            spec: &str,
        ) -> Result<Arc<CoreVersionManifest>, MockError> {
            let manifest = self.fetch_full_manifest(name).await?;
            let resolved_version = resolve_target_version((&*manifest).into(), spec)
                .map_err(|e| MockError(format!("{name}@{spec}: {e}")))?;
            manifest
                .get_core_version(&resolved_version)
                .map(Arc::new)
                .ok_or_else(|| {
                    MockError(format!(
                        "Version {resolved_version} not found in manifest for {name}"
                    ))
                })
        }
    }

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl crate::service::ManifestProvider for MockRegistryClient {
        async fn execute_manifest_job(
            &self,
            job: crate::service::ManifestJob,
        ) -> Result<crate::service::ManifestJobDone, Self::Error> {
            use crate::service::{ManifestFullData, ManifestJob, ManifestJobDone};

            match job {
                ManifestJob::Full { name, spec } => {
                    let full = self.fetch_full_manifest(&name).await?;
                    let speculative = spec.and_then(|spec| {
                        resolve_target_version((&*full).into(), &spec)
                            .ok()
                            .and_then(|version| {
                                full.get_core_version(&version)
                                    .map(|core| (spec, Arc::new(core)))
                            })
                    });
                    Ok(ManifestJobDone::Full {
                        name,
                        data: ManifestFullData::Full {
                            manifest: full,
                            speculative,
                        },
                    })
                }
                ManifestJob::Version {
                    name,
                    spec,
                    fetch_spec,
                } => {
                    let manifest = self.fetch_version_manifest(&name, &fetch_spec).await?;
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
                    let manifest =
                        full.get_core_version(&version)
                            .map(Arc::new)
                            .ok_or_else(|| {
                                MockError(format!(
                                    "Version {version} not found in manifest for {name}"
                                ))
                            })?;
                    Ok(ManifestJobDone::Version {
                        name,
                        spec,
                        manifest,
                    })
                }
            }
        }
    }
}
