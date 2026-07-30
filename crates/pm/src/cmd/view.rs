use std::collections::{BTreeMap, HashMap};

use anyhow::{Context as _, Result, anyhow};
use utoo_ruborist::manifest::VersionManifest;
use utoo_ruborist::service::{MetadataFormat, fetch_full_manifest_fresh};
use utoo_ruborist::util::parse_package_spec;

use crate::error::{CliError, classify};
use crate::model::cli_output::ErrorDetails;
use crate::model::cli_output::{ViewDist, ViewResult};
use crate::service::auth;
use crate::util::format_print::print_package_info;
use crate::util::invocation;
use crate::util::presenter::emit;
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
    let registry_started = std::time::Instant::now();
    let fetched = fetch_full_manifest_fresh(
        registry_url,
        name,
        MetadataFormat::Complete,
        token.as_deref(),
    )
    .await
    .with_context(|| format!("Failed to fetch package info for {package_spec}"));
    let (full_manifest, _etag) = match fetched {
        Ok(result) => result,
        Err(error) => {
            if !invocation::json() {
                return Err(error);
            }
            let status = error.chain().find_map(|source| {
                source
                    .downcast_ref::<reqwest::Error>()
                    .and_then(reqwest::Error::status)
                    .map(|status| status.as_u16())
            });
            return Err(CliError::new(classify(&error), format!("{error:#}"))
                .with_code("registry_request_failed")
                .with_details(ErrorDetails::Registry {
                    registry: registry_url.to_string(),
                    status,
                    duration_ms: registry_started.elapsed().as_millis() as u64,
                })
                .into());
        }
    };

    tracing::debug!("Fetched package info: {full_manifest:?}");

    // Resolve version and get full VersionManifest (with all display fields)
    let resolved_version =
        utoo_ruborist::registry::resolve_target_version((&full_manifest).into(), version_spec)
            .with_context(|| format!("Version resolution failed for {name}@{version_spec}"))?;

    let version_manifest: VersionManifest = full_manifest
        .get_full_version(&resolved_version)
        .ok_or_else(|| anyhow!("Version {} not found for {}", resolved_version, name))?;

    if !invocation::json() {
        return print_package_info(&full_manifest, &version_manifest);
    }
    let output = ViewResult {
        requested: package_spec.to_string(),
        name: version_manifest.core.name.clone(),
        version: version_manifest.core.version.clone(),
        description: version_manifest
            .description
            .clone()
            .or_else(|| full_manifest.description.clone()),
        license: version_manifest
            .core
            .license
            .clone()
            .or_else(|| full_manifest.license.clone()),
        homepage: version_manifest
            .homepage
            .clone()
            .or_else(|| full_manifest.homepage.clone()),
        dependencies: version_manifest
            .core
            .dependencies
            .as_ref()
            .map(sorted_map)
            .unwrap_or_default(),
        dist_tags: sorted_map(&full_manifest.dist_tags),
        dist: ViewDist {
            tarball: version_manifest.core.dist.tarball.clone(),
            shasum: version_manifest.core.dist.shasum.clone(),
            integrity: version_manifest.core.dist.integrity.clone(),
            file_count: version_manifest.core.dist.file_count.map(u64::from),
            unpacked_size: version_manifest.core.dist.unpacked_size,
        },
    };
    emit("view", &output, || Ok(()))
}

fn sorted_map(map: &HashMap<String, String>) -> BTreeMap<String, String> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mockito::Matcher;

    use super::*;

    #[test]
    fn json_maps_are_serialized_in_key_order() {
        let first = HashMap::from([
            ("zeta".to_string(), "1".to_string()),
            ("alpha".to_string(), "2".to_string()),
        ]);
        let second = HashMap::from([
            ("alpha".to_string(), "2".to_string()),
            ("zeta".to_string(), "1".to_string()),
        ]);

        assert_eq!(
            serde_json::to_string(&sorted_map(&first)).unwrap(),
            serde_json::to_string(&sorted_map(&second)).unwrap()
        );
        assert_eq!(
            serde_json::to_string(&sorted_map(&first)).unwrap(),
            r#"{"alpha":"2","zeta":"1"}"#
        );
    }

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
