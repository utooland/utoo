use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::cache::PACKAGE_CACHE;
use super::http_client::{fetch_full_manifest, fetch_version_manifest};
use crate::model::node::EdgeType;
use crate::util::config::get_registry_support_semver;
use crate::util::logger::log_verbose;
use crate::util::semver;

#[derive(Debug, Serialize, Deserialize)]
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

    /// Get full manifest with caching
    pub async fn get_full_manifest(name: &str) -> Result<Value> {
        // Check cache first
        let cached_info = PACKAGE_CACHE.get_full_manifests(name).await;
        let etag = cached_info.as_ref().and_then(|info| info.etag.as_deref());

        // Fetch from HTTP with etag
        match fetch_full_manifest(name, etag).await {
            Ok((data, new_etag)) => {
                // Update cache with new data
                PACKAGE_CACHE
                    .set_package_info(name, data.clone(), new_etag)
                    .await;
                Ok(data)
            }
            Err(e) if e.to_string().contains("Not modified") => {
                // Use cached data
                if let Some(cached) = cached_info {
                    log_verbose(&format!(
                        "Using cached full manifest for {name} (not modified)"
                    ));
                    Ok(cached.data)
                } else {
                    Err(anyhow::anyhow!(
                        "Received 304 Not Modified but no cached data available"
                    ))
                }
            }
            Err(e) => {
                Err(anyhow::anyhow!("Failed to get full manifest for {name}: {e}").context(e))
            }
        }
    }

    /// Get version manifest with caching
    pub async fn get_version_manifest(name: &str, version: &str) -> Result<Value> {
        // For registries that support semver, don't cache individual manifests
        // because the server handles semver resolution directly
        if get_registry_support_semver() {
            // Fetch from HTTP directly, no caching needed for semver-supporting registries
            let manifest = fetch_version_manifest(name, version).await?;
            return Ok(manifest);
        }

        // For registries that don't support semver, we need to cache specific version manifests
        // Try to load from manifest cache file first
        if let Some(manifest) = self::load_version_manifest_from_cache(name, version).await {
            log_verbose(&format!("Loaded {name}@{version} manifest from cache file"));
            return Ok(manifest);
        }

        // Normalize spec for HTTP request
        let (normalized_name, normalized_version) = Self::normalize_for_http(name, version);

        // Fetch from HTTP
        let manifest = fetch_version_manifest(&normalized_name, &normalized_version).await?;

        // Cache the result for non-semver registries
        PACKAGE_CACHE
            .cache_version_manifest(name, version, &manifest)
            .await;

        Ok(manifest)
    }

    /// Get version manifest by full versions approach with caching
    pub async fn get_version_manifest_by_full_versions(
        name: &str,
        spec: &str,
    ) -> Result<(String, Value)> {
        // Step 1: Get full manifest with caching
        let full_manifest = Self::get_full_manifest(name).await?;

        // Step 2: Extract version list and dist-tags
        let version_list: Vec<String> = match full_manifest.get("versions") {
            Some(versions) => {
                if let Some(versions_obj) = versions.as_object() {
                    versions_obj.keys().cloned().collect()
                } else if let Some(versions_array) = versions.as_array() {
                    versions_array
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                } else {
                    return Err(anyhow::anyhow!("Invalid versions format for {name}"));
                }
            }
            None => return Err(anyhow::anyhow!("No versions found for {name}")),
        };

        let dist_tags = full_manifest.get("dist-tags").cloned().unwrap_or_default();

        // Step 3: Find matching version
        let version_str = if let Some(dist_tags_obj) = dist_tags.as_object() {
            if let Some(tag_version) = dist_tags_obj.get(spec).and_then(|v| v.as_str()) {
                log_verbose(&format!(
                    "Found dist-tag {spec} -> {tag_version} for {name}"
                ));
                tag_version.to_string()
            } else {
                semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                    .map(|v| v.to_string())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No version found matching {name}@{spec}, available versions: {}",
                            version_list.join(", ")
                        )
                    })?
            }
        } else {
            semver::max_satisfying(version_list.iter().map(|s| s.as_str()), spec)
                .map(|v| v.to_string())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No version found matching {name}@{spec}, available versions: {}",
                        version_list.join(", ")
                    )
                })?
        };

        // Step 4: Get specific version manifest with caching
        let manifest = Self::get_version_manifest(name, &version_str).await?;

        log_verbose(&format!("Resolved {name}@{spec} => {version_str}"));
        Ok((version_str, manifest))
    }

    pub async fn resolve_package(name: &str, spec: &str) -> Result<ResolvedPackage> {
        log_verbose(&format!(
            "RegistryService::resolve_package starting for {name}@{spec}"
        ));

        // Check project-level cache first
        if let Some(cached_version) = PACKAGE_CACHE.get_version_in_project_cache(name, spec).await
            && let Some(cached_manifest) = PACKAGE_CACHE
                .get_manifest_in_project_cache(name, spec, &cached_version)
                .await
        {
            log_verbose(&format!(
                "Found cached resolution for {name}@{spec} => {cached_version}"
            ));
            return Ok(ResolvedPackage {
                name: name.to_string(),
                version: cached_version,
                manifest: cached_manifest,
            });
        }

        // Normalize spec for HTTP request
        let (normalized_name, normalized_version) = Self::normalize_for_http(name, spec);

        let (version, mut manifest) = if get_registry_support_semver() {
            // For supported registries, use optimized approach
            let manifest =
                Self::get_version_manifest(&normalized_name, &normalized_version).await?;
            let version = manifest
                .get("version")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            (version, manifest)
        } else {
            // Use full versions approach
            Self::get_version_manifest_by_full_versions(&normalized_name, &normalized_version)
                .await?
        };

        // Clean up optional dependencies
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

        log_verbose(&format!(
            "RegistryService::resolve_package completed for {name}@{spec} => {version}"
        ));

        // Store resolved package in project-level cache for future lookups
        PACKAGE_CACHE
            .set_manifest_in_project_cache(name, spec, &version, manifest.clone())
            .await;

        if !get_registry_support_semver() {
            let name_clone = name.to_string();
            let version_clone = version.clone();
            let manifest_clone = manifest.clone();

            tokio::spawn(async move {
                PACKAGE_CACHE
                    .cache_version_manifest(&name_clone, &version_clone, &manifest_clone)
                    .await;
            });
        }

        Ok(ResolvedPackage {
            name: name.to_string(),
            version,
            manifest,
        })
    }
}

/// Helper function to load version manifest from cache
async fn load_version_manifest_from_cache(name: &str, version: &str) -> Option<Value> {
    let manifest_file = crate::util::cache::get_package_manifest_cache_file(name, version);

    if let Ok(content) = tokio::fs::read_to_string(&manifest_file).await
        && let Ok(version_manifest) =
            serde_json::from_str::<crate::service::cache::VersionManifest>(&content)
    {
        return Some(version_manifest.manifest);
    }
    None
}

// Global resolve function
pub async fn resolve(name: &str, spec: &str) -> Result<ResolvedPackage> {
    let start_time = std::time::Instant::now();
    log_verbose(&format!("Starting resolve for {name}@{spec}"));

    let result = RegistryService::resolve_package(name, spec).await;

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

/// Get full manifest with caching
pub async fn get_full_manifest(name: &str) -> Result<Value> {
    RegistryService::get_full_manifest(name).await
}

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
