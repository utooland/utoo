use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use super::cache::{PACKAGE_CACHE, VersionsInfo};
use super::http_client::fetch_full_manifest;
use crate::model::manifest::{FullManifest, VersionManifest};
use crate::model::node::EdgeType;
use crate::service::http_client::fetch_version_manifest;
use crate::util::config::get_registry_support_semver;
use crate::util::semver;

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    #[allow(dead_code)]
    pub name: String,
    pub manifest: Value,
    pub version: String,
}

pub struct RegistryService;

impl RegistryService {
    /// Normalize spec for HTTP requests (handle npm:, workspace: prefixes)
    /// Returns (normalized_name, normalized_spec)
    fn normalize_for_http(name: &str, spec: &str) -> (String, String) {
        if spec.starts_with("npm:") {
            let npm_spec = spec.strip_prefix("npm:").unwrap();
            if let Some(last_at_index) = npm_spec.rfind('@') {
                let (pkg_name, version) = npm_spec.split_at(last_at_index);
                return (pkg_name.to_string(), version[1..].to_string());
            } else {
                return (npm_spec.to_string(), "*".to_string());
            }
        }

        if spec.starts_with("workspace:") {
            let workspace_spec = spec.strip_prefix("workspace:").unwrap();
            return (name.to_string(), workspace_spec.to_string());
        }

        (name.to_string(), spec.to_string())
    }

    /// Resolve package versions with smart caching strategy
    /// Priority: memory versions > disk versions.json > network
    /// Returns Arc<VersionsInfo> for efficient sharing without cloning
    pub async fn resolve_package_versions(name: &str) -> Result<Arc<VersionsInfo>> {
        tracing::debug!("Resolving package versions for: {name}");

        // 1. Check memory versions cache (already 304 verified)
        if let Some(versions_info_arc) = PACKAGE_CACHE.get_versions(name) {
            tracing::debug!("Using cached versions info for: {name}");
            return Ok(versions_info_arc);
        }

        // 2. Load from disk and make network request with etag
        let (etag, disk_versions) = PACKAGE_CACHE.get_versions_from_disk(name).await;
        tracing::debug!("Loaded etag from disk for {name}: {etag:?}");

        // 3. Network request with etag for 304 validation
        match fetch_full_manifest(name, etag.as_deref()).await {
            Ok((full_manifest, new_etag)) => {
                tracing::debug!("Received fresh full manifest for: {name}");

                // Convert full manifest to versions info for disk cache
                let versions_info =
                    Self::extract_versions_info_from_full_manifest(&full_manifest, new_etag);

                // Store in memory caches (sync) - take ownership, zero clone
                // set_* methods return Arc, no unwrap needed!
                PACKAGE_CACHE.set_full_manifest(name.to_string(), full_manifest);
                let versions_arc = PACKAGE_CACHE.set_versions(name.to_string(), versions_info);

                // Async disk update - Arc clone is cheap
                let name_for_disk = name.to_string();
                let versions_arc_for_disk = Arc::clone(&versions_arc);
                tokio::spawn(async move {
                    PACKAGE_CACHE
                        .set_versions_to_disk(&name_for_disk, &versions_arc_for_disk)
                        .await;
                });

                // Return Arc (zero-cost)
                Ok(versions_arc)
            }
            Err(e) if e.to_string().contains("Not modified") => {
                tracing::debug!("304 Not Modified for {name}, using disk cache");

                // 304 response means our disk versions.json is valid
                if let Some(versions_info) = disk_versions {
                    let versions_arc = PACKAGE_CACHE.set_versions(name.to_string(), versions_info);
                    Ok(versions_arc)
                } else {
                    Err(anyhow::anyhow!(
                        "Received 304 Not Modified but no disk cache available for {name}"
                    ))
                }
            }
            Err(e) => {
                tracing::debug!("Network request failed for {name}: {e}");
                Err(anyhow::anyhow!(
                    "Failed to resolve package versions for {name}: {e}"
                ))
            }
        }
    }

    /// Extract versions info from full manifest for caching
    fn extract_versions_info_from_full_manifest(
        full_manifest: &FullManifest,
        etag: Option<String>,
    ) -> VersionsInfo {
        let mut versions_data = serde_json::json!({});

        let version_list = full_manifest.versions.keys().collect::<Vec<_>>();

        // Extract essential data for versions.json
        versions_data["version_list"] = serde_json::json!(version_list);
        versions_data["dist-tags"] =
            serde_json::to_value(&full_manifest.dist_tags).unwrap_or(serde_json::json!({}));
        versions_data["time"] =
            serde_json::to_value(&full_manifest.time).unwrap_or(serde_json::json!({}));
        versions_data["name"] = serde_json::json!(full_manifest.name);

        VersionsInfo {
            versions: serde_json::from_value(versions_data).unwrap(),
            etag,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Resolve specific version manifest with three-tier caching
    /// Priority: memory > disk > network
    /// Returns Arc<VersionManifest> for efficient sharing without cloning
    pub async fn resolve_version_manifest(
        name: &str,
        version: &str,
    ) -> Result<Arc<VersionManifest>> {
        tracing::debug!("Resolving version manifest for: {name}@{version}");

        // 1. Check memory cache (sync, highest performance)
        if let Some(cached_manifest_arc) = PACKAGE_CACHE.get_version_manifest(name, version) {
            tracing::debug!("Memory cache hit for version manifest: {name}@{version}");
            return Ok(cached_manifest_arc);
        }

        // 2. Check memory by full_manifest cache (already 304 verified)
        if let Some(full_manifest_arc) = PACKAGE_CACHE.get_full_manifest(name) {
            tracing::debug!("Using cached versions info for: {name}");
            if let Some(manifest) = full_manifest_arc.versions.get(version) {
                // Update memory cache (takes ownership via clone from HashMap)
                // set_version_manifest returns Arc, no unwrap needed!
                let manifest_arc = PACKAGE_CACHE.set_version_manifest(
                    name.to_string(),
                    version.to_string(),
                    manifest.clone(), // Clone from HashMap value
                );

                // Only write to disk cache if registry doesn't support semver
                // When registry supports semver, the registry itself acts as the cache
                if !get_registry_support_semver() {
                    let name_for_disk = name.to_string();
                    let version_for_disk = version.to_string();
                    let manifest_arc_for_disk = Arc::clone(&manifest_arc);

                    // Async disk write (non-blocking)
                    // Send and forget. no result validation is performed;
                    // disk-cache are accessed only in later executions.
                    tokio::spawn(async move {
                        PACKAGE_CACHE
                            .set_version_manifest_to_disk(
                                &name_for_disk,
                                &version_for_disk,
                                &manifest_arc_for_disk,
                            )
                            .await;
                    });
                }

                // Return Arc (zero-cost)
                return Ok(manifest_arc);
            }
        }

        // 3. Check disk cache
        if let Some(cached_manifest) = PACKAGE_CACHE
            .get_version_manifest_from_disk(name, version)
            .await
        {
            tracing::debug!("Disk cache hit for version manifest: {name}@{version}");

            // Update memory cache immediately (takes ownership)
            // set_version_manifest returns Arc, no unwrap needed!
            let manifest_arc = PACKAGE_CACHE.set_version_manifest(
                name.to_string(),
                version.to_string(),
                cached_manifest,
            );

            // Return Arc (zero-cost)
            return Ok(manifest_arc);
        }

        // 4. Network request as last resort
        tracing::debug!("Cache miss, fetching from network: {name}@{version}");

        // Normalize version for HTTP request
        let (normalized_name, normalized_version) = Self::normalize_for_http(name, version);

        match fetch_version_manifest(&normalized_name, &normalized_version).await {
            Ok(manifest) => {
                tracing::debug!("Successfully fetched version manifest: {name}@{version}");

                // Update memory cache (takes ownership, zero clone)
                // set_version_manifest returns Arc, no unwrap needed!
                let manifest_arc = PACKAGE_CACHE.set_version_manifest(
                    name.to_string(),
                    version.to_string(),
                    manifest,
                );

                // Only write to disk cache if registry doesn't support semver
                // When registry supports semver, the registry itself acts as the cache
                if !get_registry_support_semver() {
                    let name_for_disk = name.to_string();
                    let version_for_disk = version.to_string();
                    let manifest_arc_for_disk = Arc::clone(&manifest_arc);

                    // Async disk write (non-blocking)
                    // Send and forget. no result validation is performed;
                    // disk-cache are accessed only in later executions.
                    tokio::spawn(async move {
                        PACKAGE_CACHE
                            .set_version_manifest_to_disk(
                                &name_for_disk,
                                &version_for_disk,
                                &manifest_arc_for_disk,
                            )
                            .await;
                    });
                }

                // Return Arc (zero-cost)
                Ok(manifest_arc)
            }
            Err(e) => {
                tracing::debug!("Failed to fetch version manifest for {name}@{version}: {e}");
                Err(anyhow::anyhow!(
                    "Failed to resolve version manifest for {name}@{version}: {e}"
                ))
            }
        }
    }

    /// Resolve target version from dist-tags and version list
    /// This is the core version resolution logic that matches npm's behavior
    fn resolve_target_version(
        dist_tags: &HashMap<String, String>,
        version_list: &[String],
        spec: &str,
    ) -> Result<String> {
        if version_list.is_empty() {
            return Err(anyhow::anyhow!("No versions available"));
        }

        // First check if spec is a dist-tag
        if let Some(version) = dist_tags.get(spec) {
            return Ok(version.to_string());
        }

        // Not a dist-tag, do semver matching
        // Check if 'latest' dist-tag satisfies the spec (npm behavior)
        let version = if let Some(latest) = dist_tags.get("latest")
            && semver::matches(spec, latest)
        {
            tracing::debug!("Using dist-tags 'latest' version {latest} for spec {spec}");
            Some(latest.to_string())
        } else {
            semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                .map(|v| v.to_string())
        };

        version.ok_or_else(|| {
            anyhow::anyhow!(
                "No matching version found for spec '{}' from {} available versions",
                spec,
                version_list.len()
            )
        })
    }

    // Note: get_version_manifest_by_full_versions has been replaced with the new cache-first architecture:
    // - get_target_version_by_cache: gets version from cache
    // - get_target_version_by_full_manifest: gets version from network
    // - get_version_manifest_by_cache: gets manifest from cache or network
    /// Main package resolution coordinator with clean caching architecture
    pub async fn resolve_package(name: &str, spec: &str) -> Result<ResolvedPackage> {
        tracing::debug!("Starting package resolution for: {name}@{spec}");

        // 1. Check project-level cache first (unchanged)
        if let Some(cached_version) = PACKAGE_CACHE.get_version_in_project_cache(name, spec).await
            && let Some(cached_manifest) = PACKAGE_CACHE
                .get_manifest_in_project_cache(name, spec, &cached_version)
                .await
        {
            tracing::debug!("Project cache hit for: {name}@{spec} => {cached_version}");
            return Ok(ResolvedPackage {
                name: name.to_string(),
                version: cached_version,
                manifest: cached_manifest,
            });
        }

        let (version, mut manifest) = if get_registry_support_semver() {
            tracing::debug!("Using semver-supporting registry for: {name}@{spec}");

            let version_manifest = Self::resolve_version_manifest(name, spec).await?;
            let version = version_manifest.version.clone();
            let manifest = serde_json::to_value(&version_manifest)?;
            (version, manifest)
        } else {
            tracing::debug!("Using non-semver registry for: {name}@{spec}");

            // 2. Resolve package versions using new caching architecture
            let versions_info_arc = Self::resolve_package_versions(name).await?;

            // 3. Resolve target version using extracted logic
            let dist_tags = &versions_info_arc.versions.dist_tags;
            let version_list = &versions_info_arc.versions.version_list;

            let target_version = Self::resolve_target_version(dist_tags, version_list, spec)?;
            tracing::debug!("Resolved target version for {name}@{spec}: {target_version}");

            // 5. Get specific version manifest using three-tier caching
            let version_manifest = Self::resolve_version_manifest(name, &target_version).await?;
            let manifest = serde_json::to_value(&version_manifest)?;
            (target_version, manifest)
        };

        // 6. Clean up optional dependencies (preserve existing logic)
        if let Some(obj) = manifest.as_object_mut()
            && let Some(optional_deps) = obj.get("optionalDependencies").and_then(|v| v.as_object())
        {
            let optional_keys: Vec<String> = optional_deps.keys().cloned().collect();
            if let Some(deps) = obj.get_mut("dependencies").and_then(|v| v.as_object_mut()) {
                for key in &optional_keys {
                    deps.remove(key);
                }
            }
            if let Some(dev_deps) = obj
                .get_mut("devDependencies")
                .and_then(|v| v.as_object_mut())
            {
                for key in &optional_keys {
                    dev_deps.remove(key);
                }
            }
        }

        // 7. Cache resolved result in project cache
        PACKAGE_CACHE
            .set_manifest_in_project_cache(name, spec, &version, manifest.clone())
            .await;

        tracing::debug!("Successfully resolved package: {name}@{spec} => {version}");

        Ok(ResolvedPackage {
            name: name.to_string(),
            version: version.to_string(),
            manifest,
        })
    }
}

// Global resolve function
pub async fn resolve(name: &str, spec: &str) -> Result<ResolvedPackage> {
    let start_time = std::time::Instant::now();
    tracing::debug!("Starting resolve for {name}@{spec}");

    let (normalized_name, normalized_version) = RegistryService::normalize_for_http(name, spec);

    let result = RegistryService::resolve_package(&normalized_name, &normalized_version).await;

    match &result {
        Ok(resolved) => {
            tracing::debug!(
                "resolve completed for {}@{} => {} in {:?}",
                name,
                spec,
                resolved.version,
                start_time.elapsed()
            );
        }
        Err(e) => {
            tracing::debug!(
                "resolve FAILED for {}@{} in {:?}: {}",
                name,
                spec,
                start_time.elapsed(),
                e
            );
        }
    }

    result
}

// Public registry API with caching

pub async fn resolve_dependency(
    name: &str,
    spec: &str,
    edge_type: &EdgeType,
) -> Result<Option<ResolvedPackage>> {
    let start_time = std::time::Instant::now();
    tracing::debug!(
        "Starting resolve_dependency for {}@{} ({})",
        name,
        spec,
        match edge_type {
            EdgeType::Prod => "prod",
            EdgeType::Dev => "dev",
            EdgeType::Peer => "peer",
            EdgeType::Optional => "optional",
        }
    );

    match resolve(name, spec).await {
        Ok(resolved) => {
            tracing::debug!(
                "resolve_dependency completed for {}@{} => {} in {:?}",
                name,
                spec,
                resolved.version,
                start_time.elapsed()
            );
            Ok(Some(resolved))
        }
        Err(e) => {
            let elapsed = start_time.elapsed();
            if *edge_type == EdgeType::Optional {
                tracing::debug!(
                    "skipping optional dependency {name}@{spec} due to resolve error after {elapsed:?}: {e}"
                );
                Ok(None)
            } else {
                tracing::debug!(
                    "resolve_dependency FAILED for {name}@{spec} after {elapsed:?}: {e}"
                );
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::cache::{Versions, VersionsInfo};
    use std::collections::HashMap;

    /// Test dist-tags matching logic
    #[tokio::test]
    async fn test_dist_tags_matching() {
        // Create test data with dist-tags
        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.2.3".to_string());
        dist_tags.insert("beta".to_string(), "2.0.0-beta.1".to_string());
        dist_tags.insert("alpha".to_string(), "2.0.0-alpha.1".to_string());
        dist_tags.insert("next".to_string(), "1.3.0".to_string());

        let versions = Versions {
            version_list: vec![
                "1.0.0".to_string(),
                "1.1.0".to_string(),
                "1.2.3".to_string(),
                "1.3.0".to_string(),
                "2.0.0-alpha.1".to_string(),
                "2.0.0-beta.1".to_string(),
            ],
            dist_tags,
        };

        let versions_info = VersionsInfo {
            versions,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        // Test cases for dist-tags matching
        let test_cases = vec![
            ("latest", "1.2.3"),
            ("beta", "2.0.0-beta.1"),
            ("alpha", "2.0.0-alpha.1"),
            ("next", "1.3.0"),
            // Test semver matching with latest priority (npm behavior)
            ("^1.0.0", "1.2.3"), // Should use latest (1.2.3) since it satisfies ^1.0.0
            ("~1.1.0", "1.1.0"), // Should match exact semver
            ("1.0.0", "1.0.0"),  // Should match exact version
        ];

        for (spec, expected_version) in test_cases {
            let result = test_resolve_with_dist_tags(&versions_info, "test-package", spec).await;
            match result {
                Ok(resolved) => {
                    assert_eq!(resolved.version, expected_version);
                    println!("✓ test-package@{} resolved to {}", spec, resolved.version);
                }
                Err(e) => {
                    panic!("Failed to resolve {}@{}: {}", "test-package", spec, e);
                }
            }
        }
    }

    /// Test edge cases for dist-tags matching
    #[tokio::test]
    async fn test_dist_tags_edge_cases() {
        // Test with empty dist-tags
        let versions_empty_dist_tags = Versions {
            version_list: vec!["1.0.0".to_string(), "1.1.0".to_string()],
            dist_tags: HashMap::new(),
        };

        let versions_info_empty = VersionsInfo {
            versions: versions_empty_dist_tags,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        // Should fall back to semver matching
        let result =
            test_resolve_with_dist_tags(&versions_info_empty, "test-package", "^1.0.0").await;
        match result {
            Ok(resolved) => {
                assert_eq!(resolved.version, "1.1.0"); // Should match highest semver
                println!("✓ Empty dist-tags fallback to semver: {}", resolved.version);
            }
            Err(e) => {
                panic!("Failed to resolve with empty dist-tags: {e}");
            }
        }

        // Test with invalid dist-tag
        let mut dist_tags = HashMap::new();
        dist_tags.insert("invalid-tag".to_string(), "1.0.0".to_string());

        let versions = Versions {
            version_list: vec!["1.0.0".to_string(), "1.1.0".to_string()],
            dist_tags,
        };

        let versions_info = VersionsInfo {
            versions,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        // Should fall back to semver matching for non-existent dist-tag
        let result = test_resolve_with_dist_tags(&versions_info, "test-package", "^1.0.0").await;
        match result {
            Ok(resolved) => {
                assert_eq!(resolved.version, "1.1.0"); // Should match highest semver
                println!(
                    "✓ Non-existent dist-tag fallback to semver: {}",
                    resolved.version
                );
            }
            Err(e) => {
                panic!("Failed to resolve with non-existent dist-tag: {e}");
            }
        }
    }

    /// Test semver matching fallback
    #[tokio::test]
    async fn test_semver_fallback() {
        let versions = Versions {
            version_list: vec![
                "1.0.0".to_string(),
                "1.1.0".to_string(),
                "1.2.0".to_string(),
                "2.0.0".to_string(),
                "2.1.0".to_string(),
            ],
            dist_tags: HashMap::new(),
        };

        let versions_info = VersionsInfo {
            versions,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        let test_cases = vec![
            ("^1.0.0", "1.2.0"),         // Should match highest 1.x
            ("~1.1.0", "1.1.0"),         // Should match exact 1.1.x
            (">=1.0.0 <2.0.0", "1.2.0"), // Should match highest in range
            ("2.x", "2.1.0"),            // Should match highest 2.x
        ];

        for (spec, expected_version) in test_cases {
            let result = test_resolve_with_dist_tags(&versions_info, "test-package", spec).await;
            match result {
                Ok(resolved) => {
                    assert_eq!(resolved.version, expected_version);
                    println!(
                        "✓ Semver fallback test-package@{} resolved to {}",
                        spec, resolved.version
                    );
                }
                Err(e) => {
                    panic!("Failed to resolve {}@{}: {}", "test-package", spec, e);
                }
            }
        }
    }

    /// Test that 'latest' dist-tag is preferred when it satisfies the semver requirement
    #[tokio::test]
    async fn test_latest_dist_tag_priority() {
        // Create a scenario where:
        // - latest points to 1.5.0
        // - version_list has 1.0.0, 1.5.0, 1.9.0
        // - User requests ^1.0.0
        // Expected: Should use 1.5.0 (latest) even though 1.9.0 is the max_satisfying version

        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.5.0".to_string());

        let versions = Versions {
            version_list: vec![
                "1.0.0".to_string(),
                "1.5.0".to_string(),
                "1.9.0".to_string(),
            ],
            dist_tags: dist_tags.clone(),
        };

        let versions_info = VersionsInfo {
            versions,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        // Test: ^1.0.0 should use latest (1.5.0) instead of max_satisfying (1.9.0)
        let result = test_resolve_with_dist_tags(&versions_info, "test-package", "^1.0.0").await;
        match result {
            Ok(resolved) => {
                assert_eq!(resolved.version, "1.5.0");
                println!("✓ Latest dist-tag (1.5.0) was preferred over max_satisfying (1.9.0) for ^1.0.0");
            }
            Err(e) => {
                panic!("Failed to resolve with latest dist-tag priority: {e}");
            }
        }

        // Test: ^2.0.0 should use max_satisfying since latest doesn't satisfy
        let mut dist_tags2 = HashMap::new();
        dist_tags2.insert("latest".to_string(), "1.5.0".to_string());

        let versions2 = Versions {
            version_list: vec![
                "1.5.0".to_string(),
                "2.0.0".to_string(),
                "2.1.0".to_string(),
            ],
            dist_tags: dist_tags2,
        };

        let versions_info2 = VersionsInfo {
            versions: versions2,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        let result2 = test_resolve_with_dist_tags(&versions_info2, "test-package", "^2.0.0").await;
        match result2 {
            Ok(resolved) => {
                assert_eq!(resolved.version, "2.1.0");
                println!("✓ Latest (1.5.0) doesn't satisfy ^2.0.0, correctly used max_satisfying (2.1.0)");
            }
            Err(e) => {
                panic!("Failed to resolve when latest doesn't satisfy: {e}");
            }
        }

        // Test: No latest tag should fall back to max_satisfying
        let versions3 = Versions {
            version_list: vec![
                "1.0.0".to_string(),
                "1.5.0".to_string(),
                "1.9.0".to_string(),
            ],
            dist_tags: HashMap::new(),
        };

        let versions_info3 = VersionsInfo {
            versions: versions3,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        let result3 = test_resolve_with_dist_tags(&versions_info3, "test-package", "^1.0.0").await;
        match result3 {
            Ok(resolved) => {
                assert_eq!(resolved.version, "1.9.0");
                println!("✓ No latest tag, correctly used max_satisfying (1.9.0)");
            }
            Err(e) => {
                panic!("Failed to resolve without latest tag: {e}");
            }
        }
    }

    /// Test error cases
    #[tokio::test]
    async fn test_resolve_errors() {
        let versions = Versions {
            version_list: vec!["1.0.0".to_string()],
            dist_tags: HashMap::new(),
        };

        let versions_info = VersionsInfo {
            versions,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        // Test with invalid semver that should fail
        let result = test_resolve_with_dist_tags(&versions_info, "test-package", "^2.0.0").await;
        assert!(result.is_err(), "Should fail for incompatible semver range");
        println!("✓ Correctly failed for incompatible semver range");

        // Test with empty version list
        let empty_versions = Versions {
            version_list: vec![],
            dist_tags: HashMap::new(),
        };

        let empty_versions_info = VersionsInfo {
            versions: empty_versions,
            etag: Some("test-etag".to_string()),
            last_updated: 1234567890,
        };

        let result =
            test_resolve_with_dist_tags(&empty_versions_info, "test-package", "^1.0.0").await;
        assert!(result.is_err(), "Should fail for empty version list");
        println!("✓ Correctly failed for empty version list");
    }

    /// Helper function to test dist-tags matching logic
    /// This directly calls the real resolve_target_version method
    async fn test_resolve_with_dist_tags(
        versions_info: &VersionsInfo,
        name: &str,
        spec: &str,
    ) -> Result<ResolvedPackage> {
        let dist_tags = &versions_info.versions.dist_tags;
        let version_list = &versions_info.versions.version_list;

        // Call the real method from RegistryService
        let target_version = RegistryService::resolve_target_version(dist_tags, version_list, spec)?;

        // Create a mock resolved package
        Ok(ResolvedPackage {
            name: name.to_string(),
            version: target_version.clone(),
            manifest: serde_json::json!({
                "name": name,
                "version": target_version,
                "dependencies": {}
            }),
        })
    }
}
