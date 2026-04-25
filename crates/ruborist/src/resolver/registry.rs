//! Registry resolution functions.
//!
//! This module provides the unified package resolution logic that combines
//! registry fetching with version resolution.
//!
//! The resolution automatically adapts to registry capabilities:
//! - For semver-supporting registries: directly fetches specific version
//! - For traditional registries: fetches full manifest and resolves locally

use crate::model::manifest::FullManifest;
use crate::model::node::EdgeType;
use crate::resolver::semver::normalize_spec;
use crate::resolver::version::resolve_target_version;
use crate::traits::registry::{RegistryClient, ResolvedPackage};

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
pub async fn resolve_package<R: RegistryClient + Sync>(
    registry: &R,
    name: &str,
    spec: &str,
) -> Result<ResolvedPackage, ResolveError<R::Error>> {
    // Normalize spec first to handle npm: alias and workspace: prefix
    // This ensures correct behavior even if RegistryClient::resolve_package is overridden
    // e.g., "wrap-ansi-cjs" + "npm:wrap-ansi@^7.0.0" -> fetch "wrap-ansi" @ "^7.0.0"
    let (fetch_name, fetch_spec) = normalize_spec(name, spec);

    registry
        .resolve_package(&fetch_name, &fetch_spec)
        .await
        .map_err(ResolveError::Registry)
}

/// Resolve a package from already-fetched manifest.
///
/// Useful when you already have the manifest and want to resolve a version.
pub fn resolve_from_manifest<E: std::error::Error + 'static>(
    manifest: &FullManifest,
    spec: &str,
) -> Result<ResolvedPackage, ResolveError<E>> {
    let version_list: Vec<String> = manifest.versions.keys.clone();

    if version_list.is_empty() {
        return Err(ResolveError::NoVersions(manifest.name.clone()));
    }

    // Resolve version using shared logic
    let resolved_version = resolve_target_version(&manifest.dist_tags, &version_list, spec)
        .map_err(|e| ResolveError::Version(format!("{}@{}: {}", manifest.name, spec, e)))?;

    // O(1) lookup from pre-parsed `Versions` map.
    let version_manifest = manifest
        .get_core_version(&resolved_version)
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
pub async fn resolve_registry_dep<R: RegistryClient + Sync>(
    registry: &R,
    name: &str,
    spec: &str,
    edge_type: &EdgeType,
) -> Result<Option<ResolvedPackage>, ResolveError<R::Error>> {
    match resolve_package(registry, name, spec).await {
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
