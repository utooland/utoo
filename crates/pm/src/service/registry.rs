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
use crate::util::logger::log_verbose;
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

/// Result types for new caching architecture
#[derive(Debug, Clone)]
pub enum PackageVersionsResult {
    /// From memory cache, already 304 verified
    Cached(VersionsInfo),
    /// New network data (200 response)
    Fresh(VersionsInfo),
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
    /// Priority: memory full-manifest > memory versions > disk versions.json > network
    pub async fn resolve_package_versions(name: &str) -> Result<PackageVersionsResult> {
        log_verbose(&format!("Resolving package versions for: {name}"));

        // 1. Check memory full-manifest cache (highest priority, already 304 verified)
        if let Some((_etag, versions_info)) = PACKAGE_CACHE.get_versions(name) {
            log_verbose(&format!("Using cached full manifest for versions: {name}"));
            return Ok(PackageVersionsResult::Fresh(versions_info));
        }

        // 2. Check memory versions cache (already 304 verified)
        if let Some((_etag, cached_versions)) = PACKAGE_CACHE.get_versions(name) {
            log_verbose(&format!("Using cached versions info for: {name}"));
            return Ok(PackageVersionsResult::Cached(cached_versions));
        }

        // 3. Load from disk and make network request with etag
        let (etag, disk_versions) = PACKAGE_CACHE.get_versions_from_disk(name).await;
        log_verbose(&format!("Loaded etag from disk for {name}: {etag:?}"));

        // 4. Network request with etag for 304 validation
        match fetch_full_manifest(name, etag.as_deref()).await {
            Ok((full_manifest, new_etag)) => {
                log_verbose(&format!("Received fresh full manifest for: {name}"));

                // Store in memory full-manifest cache (sync)
                // For version_manifest fetch
                PACKAGE_CACHE.set_full_manifest(name, &full_manifest);

                // Convert full manifest to versions info for disk cache
                let versions_info =
                    Self::extract_versions_info_from_full_manifest(&full_manifest, new_etag);

                let versions_arc = Arc::new(versions_info.clone());
                PACKAGE_CACHE.set_versions(name, versions_arc);

                // Async disk update
                let versions_info_for_disk = versions_info.clone();
                let name_for_disk = name.to_string();
                tokio::spawn(async move {
                    PACKAGE_CACHE
                        .set_versions_to_disk(&name_for_disk, &versions_info_for_disk)
                        .await;
                });

                Ok(PackageVersionsResult::Fresh(versions_info))
            }
            Err(e) if e.to_string().contains("Not modified") => {
                log_verbose(&format!("304 Not Modified for {name}, using disk cache"));

                // 304 response means our disk versions.json is valid
                if let Some(versions_info) = disk_versions {
                    let versions_arc = Arc::new(versions_info.clone());
                    PACKAGE_CACHE.set_versions(name, versions_arc);
                    Ok(PackageVersionsResult::Cached(versions_info))
                } else {
                    Err(anyhow::anyhow!(
                        "Received 304 Not Modified but no disk cache available for {name}"
                    ))
                }
            }
            Err(e) => {
                log_verbose(&format!("Network request failed for {name}: {e}"));
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
    pub async fn resolve_version_manifest(name: &str, version: &str) -> Result<VersionManifest> {
        log_verbose(&format!("Resolving version manifest for: {name}@{version}"));

        // 1. Check memory cache (sync, highest performance)
        if let Some(cached_manifest) = PACKAGE_CACHE.get_version_manifest(name, version) {
            log_verbose(&format!(
                "Memory cache hit for version manifest: {name}@{version}"
            ));
            return Ok(cached_manifest);
        }

        // 2. Check memory by full_manifest cache (already 304 verified)
        if let Some(full_manifest) = PACKAGE_CACHE.get_full_manifest(name) {
            log_verbose(&format!("Using cached versions info for: {name}"));
            let manifest_res = full_manifest.versions.get(version).cloned();
            if let Some(manifest) = manifest_res {
                PACKAGE_CACHE.set_version_manifest(name, version, &manifest);
                // Async disk write (non-blocking)
                let name_clone = name.to_string();
                let version_clone = version.to_string();
                let manifest_clone = manifest.clone();

                tokio::spawn(async move {
                    PACKAGE_CACHE
                        .set_version_manifest_to_disk(&name_clone, &version_clone, &manifest_clone)
                        .await;
                });
                return Ok(manifest);
            }
        }

        // 3. Check disk cache
        if let Some(cached_manifest) = PACKAGE_CACHE
            .get_version_manifest_from_disk(name, version)
            .await
        {
            log_verbose(&format!(
                "Disk cache hit for version manifest: {name}@{version}"
            ));

            // Update memory cache immediately (sync)
            PACKAGE_CACHE.set_version_manifest(name, version, &cached_manifest);
            return Ok(cached_manifest);
        }

        // 4. Network request as last resort
        log_verbose(&format!(
            "Cache miss, fetching from network: {name}@{version}"
        ));

        // Normalize version for HTTP request
        let (normalized_name, normalized_version) = Self::normalize_for_http(name, version);

        match fetch_version_manifest(&normalized_name, &normalized_version).await {
            Ok(manifest) => {
                log_verbose(&format!(
                    "Successfully fetched version manifest: {name}@{version}"
                ));

                // 1. Update memory cache immediately (sync)
                PACKAGE_CACHE.set_version_manifest(name, version, &manifest);

                // 2. Async disk write (non-blocking)
                let name_clone = name.to_string();
                let version_clone = version.to_string();
                let manifest_clone = manifest.clone();

                tokio::spawn(async move {
                    PACKAGE_CACHE
                        .set_version_manifest_to_disk(&name_clone, &version_clone, &manifest_clone)
                        .await;
                });

                Ok(manifest)
            }
            Err(e) => {
                log_verbose(&format!(
                    "Failed to fetch version manifest for {name}@{version}: {e}"
                ));
                Err(anyhow::anyhow!(
                    "Failed to resolve version manifest for {name}@{version}: {e}"
                ))
            }
        }
    }

    // Note: get_version_manifest_by_full_versions has been replaced with the new cache-first architecture:
    // - get_target_version_by_cache: gets version from cache
    // - get_target_version_by_full_manifest: gets version from network
    // - get_version_manifest_by_cache: gets manifest from cache or network
    /// Main package resolution coordinator with clean caching architecture
    pub async fn resolve_package(name: &str, spec: &str) -> Result<ResolvedPackage> {
        log_verbose(&format!("Starting package resolution for: {name}@{spec}"));

        // 1. Check project-level cache first (unchanged)
        if let Some(cached_version) = PACKAGE_CACHE.get_version_in_project_cache(name, spec).await
            && let Some(cached_manifest) = PACKAGE_CACHE
                .get_manifest_in_project_cache(name, spec, &cached_version)
                .await
        {
            log_verbose(&format!(
                "Project cache hit for: {name}@{spec} => {cached_version}"
            ));
            return Ok(ResolvedPackage {
                name: name.to_string(),
                version: cached_version,
                manifest: cached_manifest,
            });
        }

        let (version, mut manifest) = if get_registry_support_semver() {
            log_verbose(&format!(
                "Using semver-supporting registry for: {name}@{spec}"
            ));

            let version_manifest = Self::resolve_version_manifest(name, spec).await?;
            let version = version_manifest.version.clone();
            let manifest = serde_json::to_value(&version_manifest)?;
            (version, manifest)
        } else {
            log_verbose(&format!("Using non-semver registry for: {name}@{spec}"));

            // 2. Resolve package versions using new caching architecture
            let package_versions_result = Self::resolve_package_versions(name).await?;

            let versions_info = match package_versions_result {
                PackageVersionsResult::Cached(versions_info) => versions_info,
                PackageVersionsResult::Fresh(versions_info) => versions_info,
            };

            // 3. Check dist-tags
            let dist_tags = versions_info.versions.dist_tags;
            let version_list = versions_info.versions.version_list;

            let target_version = match dist_tags.get(spec) {
                Some(version) => version.to_string(),
                None => match semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                {
                    Some(version) => version.to_string(),
                    None => {
                        log_verbose(&format!(
                            "No matching version found for {}@{} from {} available versions",
                            name,
                            spec,
                            version_list.len()
                        ));
                        return Err(anyhow::anyhow!(
                            "No matching version found for {name}@{spec}"
                        ));
                    }
                },
            };

            log_verbose(&format!(
                "Resolved target version for {name}@{spec}: {target_version}"
            ));

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

        log_verbose(&format!(
            "Successfully resolved package: {name}@{spec} => {version}"
        ));

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
    log_verbose(&format!("Starting resolve for {name}@{spec}"));

    let (normalized_name, normalized_version) = RegistryService::normalize_for_http(name, spec);

    let result = RegistryService::resolve_package(&normalized_name, &normalized_version).await;

    match &result {
        Ok(resolved) => {
            log_verbose(&format!(
                "resolve completed for {}@{} => {} in {:?}",
                name,
                spec,
                resolved.version,
                start_time.elapsed()
            ));
        }
        Err(e) => {
            log_verbose(&format!(
                "resolve FAILED for {}@{} in {:?}: {}",
                name,
                spec,
                start_time.elapsed(),
                e
            ));
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
    log_verbose(&format!(
        "Starting resolve_dependency for {}@{} ({})",
        name,
        spec,
        match edge_type {
            EdgeType::Prod => "prod",
            EdgeType::Dev => "dev",
            EdgeType::Peer => "peer",
            EdgeType::Optional => "optional",
        }
    ));

    match resolve(name, spec).await {
        Ok(resolved) => {
            log_verbose(&format!(
                "resolve_dependency completed for {}@{} => {} in {:?}",
                name,
                spec,
                resolved.version,
                start_time.elapsed()
            ));
            Ok(Some(resolved))
        }
        Err(e) => {
            let elapsed = start_time.elapsed();
            if *edge_type == EdgeType::Optional {
                log_verbose(&format!(
                    "skipping optional dependency {name}@{spec} due to resolve error after {elapsed:?}: {e}"
                ));
                Ok(None)
            } else {
                log_verbose(&format!(
                    "resolve_dependency FAILED for {name}@{spec} after {elapsed:?}: {e}"
                ));
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
            // Test semver fallback when dist-tag doesn't exist
            ("^1.0.0", "1.3.0"), // Should match highest semver (1.3.0 is higher than 1.2.3)
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
    /// This simulates the logic from RegistryService::resolve_package
    async fn test_resolve_with_dist_tags(
        versions_info: &VersionsInfo,
        name: &str,
        spec: &str,
    ) -> Result<ResolvedPackage> {
        let dist_tags = &versions_info.versions.dist_tags;
        let version_list = &versions_info.versions.version_list;

        if version_list.is_empty() {
            return Err(anyhow::anyhow!("No versions found for package: {name}"));
        }

        let target_version = match dist_tags.get(spec) {
            Some(version) => version.to_string(),
            None => match semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec) {
                Some(version) => version.to_string(),
                None => {
                    return Err(anyhow::anyhow!(
                        "No matching version found for {name}@{spec}"
                    ));
                }
            },
        };

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
