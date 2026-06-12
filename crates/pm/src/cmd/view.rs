use anyhow::{Context as _, Result, anyhow};
use utoo_ruborist::manifest::VersionManifest;
use utoo_ruborist::service::{MetadataFormat, fetch_full_manifest_fresh};
use utoo_ruborist::util::parse_package_spec;

use crate::service::auth;
use crate::util::format_print::print_package_info;
use crate::util::user_config::get_registry;

/// View package information from registry, similar to npm view
pub async fn view(package_spec: &str) -> Result<()> {
    let registry_url = get_registry();
    view_with_registry(package_spec, &registry_url).await
}

async fn view_with_registry(package_spec: &str, registry_url: &str) -> Result<()> {
    tracing::debug!("Viewing package: {package_spec}");

    // Parse package specification
    let (name, version_spec) = parse_package_spec(package_spec);

    tracing::debug!("Resolved package: {name} (spec: {version_spec})");

    // Fetch full manifest directly from registry (Complete format for display, no ETag).
    // token_for_url applies the leak guard: a token only when registry_url is
    // the configured registry host.
    let token = auth::token_for_url(registry_url).await;
    let (full_manifest, _etag) = fetch_full_manifest_fresh(
        registry_url,
        name,
        MetadataFormat::Complete,
        token.as_deref(),
    )
    .await
    .with_context(|| format!("Failed to fetch package info for {package_spec}"))?;

    tracing::debug!("Fetched package info: {full_manifest:?}");

    // Resolve version and get full VersionManifest (with all display fields)
    let resolved_version =
        utoo_ruborist::registry::resolve_target_version((&full_manifest).into(), version_spec)
            .with_context(|| format!("Version resolution failed for {name}@{version_spec}"))?;

    let version_manifest: VersionManifest = full_manifest
        .get_full_version(&resolved_version)
        .ok_or_else(|| anyhow!("Version {} not found for {}", resolved_version, name))?;

    // Print package information in npm view format
    print_package_info(&full_manifest, &version_manifest)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use mockito::Matcher;

    use super::*;

    /// E2E test: verify that calling view twice works correctly.
    /// Previously, the second call would fail with "304 Not Modified" error
    /// because the registry service used ETag caching.
    #[tokio::test]
    async fn test_view_twice_no_304_error() {
        let mut server = mockito::Server::new_async().await;
        let manifest = r#"{
            "name": "is-odd",
            "description": "mock package",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "is-odd",
                    "version": "1.0.0",
                    "description": "mock package",
                    "dist": {}
                }
            }
        }"#;
        let mock = server
            .mock("GET", "/is-odd")
            .match_header("accept", "application/json")
            .match_header("if-none-match", Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("etag", "\"mock-etag\"")
            .with_body(manifest)
            .expect(2)
            .create_async()
            .await;

        // First view - should succeed
        let result1 = view_with_registry("is-odd", &server.url()).await;
        assert!(result1.is_ok(), "First view failed: {:?}", result1.err());

        // Second view - should also succeed (not fail with 304 error)
        let result2 = view_with_registry("is-odd", &server.url()).await;
        assert!(result2.is_ok(), "Second view failed: {:?}", result2.err());
        mock.assert_async().await;
    }
}
