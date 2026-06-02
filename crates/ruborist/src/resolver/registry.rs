//! Registry resolution functions.
//!
//! This module provides the unified package resolution logic that combines
//! registry fetching with version resolution.
//!
//! The resolution automatically adapts to registry capabilities:
//! - For semver-supporting registries: directly fetches specific version
//! - For traditional registries: fetches full manifest and resolves locally

use std::sync::Arc;

use crate::model::manifest::{CoreVersionManifest, FullManifest};
use crate::model::node::EdgeType;
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::service::{ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider};
use crate::traits::registry::ResolvedPackage;

/// Error type for package resolution.
#[derive(Debug)]
pub enum ResolveError<E> {
    /// Registry fetch failed
    Registry(E),
    /// Version resolution failed
    Version(String),
    /// Package has no versions
    NoVersions(String),
    /// Resolved version not found in manifest
    ManifestNotFound { name: String, version: String },
    /// Git resolution failed
    Git { url: String, source: anyhow::Error },
    /// HTTP tarball resolution failed
    Http { url: String, source: anyhow::Error },
    /// Local `file:` resolution failed
    File { spec: String, source: anyhow::Error },
    /// Dependency type not yet supported (e.g. local file path)
    Unsupported { spec: String, reason: &'static str },
    /// Error augmented with the dependency chain that led to the failing dep.
    ///
    /// `chain` is ordered root → immediate parent as resolved `(name, version)`
    /// pairs; the failing dep's `(name, spec)` is appended as the final entry.
    /// (Ancestor entries carry resolved versions; the last entry carries the
    /// unresolved requested spec.)
    ///
    /// `Display` intentionally does not render `chain` — CLI consumers render
    /// it via downcast (see `pm::util::format_print::format_resolve_chain`).
    WithChain {
        chain: Vec<(String, String)>,
        source: Box<ResolveError<E>>,
    },
}

impl<E: std::fmt::Display> std::fmt::Display for ResolveError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Registry(e) => write!(f, "Registry error: {}", e),
            ResolveError::Version(msg) => write!(f, "Version resolution failed: {}", msg),
            ResolveError::NoVersions(name) => write!(f, "No versions available for {}", name),
            ResolveError::ManifestNotFound { name, version } => {
                write!(f, "Manifest not found for {}@{}", name, version)
            }
            ResolveError::Git { url, source } => {
                write!(f, "Git resolution failed for {}: {}", url, source)
            }
            ResolveError::Http { url, source } => {
                write!(f, "HTTP tarball resolution failed for {}: {}", url, source)
            }
            ResolveError::File { spec, source } => {
                write!(f, "File resolution failed for '{spec}': {source}")
            }
            ResolveError::Unsupported { spec, reason } => {
                write!(f, "Unsupported dependency '{spec}': {reason}")
            }
            // Display delegates to the wrapped error. The `chain` payload is
            // structured data meant for CLI renderers (pm's `format_print`) to
            // decorate — keeping presentation concerns out of the library.
            ResolveError::WithChain { source, .. } => write!(f, "{source}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ResolveError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::Registry(e) => Some(e),
            ResolveError::Version(_)
            | ResolveError::NoVersions(_)
            | ResolveError::ManifestNotFound { .. }
            | ResolveError::Unsupported { .. } => None,
            ResolveError::Git { source, .. }
            | ResolveError::Http { source, .. }
            | ResolveError::File { source, .. } => Some(source.as_ref()),
            ResolveError::WithChain { source, .. } => Some(source.as_ref()),
        }
    }
}

/// Resolve a package by name and version spec.
///
/// This is the main entry point for package resolution. It automatically
/// chooses the best strategy based on registry capabilities:
///
/// - If registry supports semver resolution, uses direct version fetch
/// - Otherwise, fetches full manifest and resolves locally
///
/// Handles npm alias specs like `npm:package@version` by fetching the aliased package.
///
/// # Arguments
/// * `registry` - Registry client for fetching metadata
/// * `name` - Package name
/// * `spec` - Version specification (semver range, dist-tag, exact version, or npm alias)
///
/// # Example
/// ```ignore
/// let resolved = resolve_package(&registry, "lodash", "^4.0.0").await?;
/// println!("Resolved to {}@{}", resolved.name, resolved.version);
/// ```
pub async fn resolve_package<P: ManifestProvider>(
    provider: &P,
    name: &str,
    spec: &str,
) -> Result<ResolvedPackage, ResolveError<P::Error>> {
    // Normalize spec first to handle npm: alias and workspace: prefix, e.g.
    // "wrap-ansi-cjs" + "npm:wrap-ansi@^7.0.0" -> fetch "wrap-ansi" @ "^7.0.0".
    let (fetch_name, fetch_spec) = normalize_spec(name, spec);

    if provider.supports_semver_resolution() {
        // Semver registries resolve the range/tag server-side: one version job
        // returns the matching version's manifest directly.
        let manifest = version_manifest(provider, &fetch_name, &fetch_spec, &fetch_spec).await?;
        return Ok(resolved_from_version(&fetch_name, manifest));
    }

    // Non-semver: fetch the full manifest, resolve the version client-side.
    let done = provider
        .execute_manifest_job(ManifestJob::Full {
            name: fetch_name.clone(),
            spec: Some(fetch_spec.clone()),
        })
        .await
        .map_err(ResolveError::Registry)?;
    match done {
        // The provider speculatively extracted the requested version already.
        ManifestJobDone::Version { manifest, .. } => {
            Ok(resolved_from_version(&fetch_name, manifest))
        }
        ManifestJobDone::Full { data, .. } => match data {
            ManifestFullData::Full { manifest, .. } => {
                let resolved = resolve_from_manifest::<P::Error>(&manifest, &fetch_spec)?;
                Ok(ResolvedPackage {
                    name: fetch_name,
                    ..resolved
                })
            }
            ManifestFullData::Versions(versions) => {
                // 304 path: resolve a concrete version from the cached list, then
                // fetch that exact version manifest.
                let version = resolve_target_version((&*versions).into(), &fetch_spec)
                    .map_err(|e| ResolveError::Version(format!("{name}@{fetch_spec}: {e}")))?;
                let manifest =
                    version_manifest(provider, &fetch_name, &version, &fetch_spec).await?;
                Ok(resolved_from_version(&fetch_name, manifest))
            }
        },
    }
}

/// Fetch a single version manifest job and unwrap it to the version manifest.
async fn version_manifest<P: ManifestProvider>(
    provider: &P,
    name: &str,
    fetch_spec: &str,
    requested_spec: &str,
) -> Result<Arc<CoreVersionManifest>, ResolveError<P::Error>> {
    let done = provider
        .execute_manifest_job(ManifestJob::Version {
            name: name.to_string(),
            spec: fetch_spec.to_string(),
            fetch_spec: fetch_spec.to_string(),
        })
        .await
        .map_err(ResolveError::Registry)?;
    match done {
        ManifestJobDone::Version { manifest, .. } => Ok(manifest),
        ManifestJobDone::Full { .. } => Err(ResolveError::Version(format!(
            "{name}@{requested_spec}: provider returned a full manifest for a version job"
        ))),
    }
}

fn resolved_from_version(name: &str, manifest: Arc<CoreVersionManifest>) -> ResolvedPackage {
    ResolvedPackage {
        name: name.to_string(),
        version: manifest.version.clone(),
        manifest,
    }
}

/// Resolve a package from already-fetched manifest.
///
/// Useful when you already have the manifest and want to resolve a version.
pub fn resolve_from_manifest<E: std::error::Error + 'static>(
    manifest: &FullManifest,
    spec: &str,
) -> Result<ResolvedPackage, ResolveError<E>> {
    if manifest.versions.is_empty() {
        return Err(ResolveError::NoVersions(manifest.name.clone()));
    }

    // Resolve version using shared logic
    let resolved_version = resolve_target_version(manifest.into(), spec)
        .map_err(|e| ResolveError::Version(format!("{}@{}: {}", manifest.name, spec, e)))?;

    // Get manifest for resolved version (lazy: parse from Value on demand)
    let version_manifest = manifest
        .get_core_version(&resolved_version)
        .map(Arc::new)
        .ok_or_else(|| ResolveError::ManifestNotFound {
            name: manifest.name.clone(),
            version: resolved_version.clone(),
        })?;

    Ok(ResolvedPackage {
        name: manifest.name.clone(),
        version: resolved_version,
        manifest: version_manifest,
    })
}

/// Resolve a registry dependency with edge type awareness.
///
/// For optional dependencies, returns `Ok(None)` on resolution failure
/// instead of propagating the error.
pub async fn resolve_registry_dep<P: ManifestProvider>(
    provider: &P,
    name: &str,
    spec: &str,
    edge_type: &EdgeType,
) -> Result<Option<ResolvedPackage>, ResolveError<P::Error>> {
    match resolve_package(provider, name, spec).await {
        Ok(resolved) => Ok(Some(resolved)),
        Err(e) => {
            if *edge_type == EdgeType::Optional {
                tracing::debug!("Skipping optional dependency {}@{}: {}", name, spec, e);
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::manifest::CoreVersionManifest;
    use crate::traits::registry::mock::MockRegistryClient;

    fn create_version_manifest(name: &str, version: &str) -> CoreVersionManifest {
        CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_resolve_package_exact() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "lodash",
            "4.17.21",
            create_version_manifest("lodash", "4.17.21"),
        );

        let resolved = resolve_package(&registry, "lodash", "4.17.21")
            .await
            .unwrap();
        assert_eq!(resolved.name, "lodash");
        assert_eq!(resolved.version, "4.17.21");
    }

    #[tokio::test]
    async fn test_resolve_package_latest() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "lodash",
            "4.17.21",
            create_version_manifest("lodash", "4.17.21"),
        );

        let resolved = resolve_package(&registry, "lodash", "latest")
            .await
            .unwrap();
        assert_eq!(resolved.version, "4.17.21");
    }

    #[tokio::test]
    async fn test_resolve_package_range() {
        let mut registry = MockRegistryClient::new();
        registry.add_package(
            "lodash",
            "4.17.21",
            create_version_manifest("lodash", "4.17.21"),
        );
        registry.add_package(
            "lodash",
            "4.17.20",
            create_version_manifest("lodash", "4.17.20"),
        );

        let resolved = resolve_package(&registry, "lodash", "^4.17.0")
            .await
            .unwrap();
        // Should prefer latest (4.17.21) since it satisfies the range
        assert_eq!(resolved.version, "4.17.21");
    }

    #[tokio::test]
    async fn test_resolve_optional_dependency_failure() {
        let registry = MockRegistryClient::new();

        let result =
            resolve_registry_dep(&registry, "nonexistent", "^1.0.0", &EdgeType::Optional).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_resolve_prod_dependency_failure() {
        let registry = MockRegistryClient::new();

        let result =
            resolve_registry_dep(&registry, "nonexistent", "^1.0.0", &EdgeType::Prod).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_with_chain_display_delegates_to_source() {
        // Display is data-only: delegates to the inner error. CLI presentation
        // of the chain lives in the pm crate, not here.
        let inner: ResolveError<std::io::Error> =
            ResolveError::NoVersions("@antskill/tegg-agent".to_string());
        let wrapped: ResolveError<std::io::Error> = ResolveError::WithChain {
            chain: vec![
                ("my-app".to_string(), "1.0.0".to_string()),
                ("@antskill/tegg-agent".to_string(), "^1.0.0".to_string()),
            ],
            source: Box::new(inner),
        };

        assert_eq!(
            wrapped.to_string(),
            "No versions available for @antskill/tegg-agent"
        );
    }
}
