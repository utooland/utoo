use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::http_client::get_version_manifest_by_full_versions;
use crate::model::node::EdgeType;
use crate::service::http_client::get_version_manifest;
use crate::util::config::get_registry_support_semver;
use crate::util::logger::log_verbose;

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
    pub async fn resolve_package(name: &str, spec: &str) -> Result<ResolvedPackage> {
        log_verbose(&format!(
            "🔍 RegistryService::resolve_package starting for {name}@{spec}"
        ));

        let mut manifest = if get_registry_support_semver() {
            // For supported registries, we'll implement semver-optimized approach later
            // For now, use the full versions approach
            get_version_manifest(name, spec).await?
        } else {
            get_version_manifest_by_full_versions(name, spec).await?
        };

        let version = manifest.get("version").unwrap().as_str().unwrap().to_string();

        log_verbose(&format!("Resolved {name}@{spec} => {version}"));

        if let Some(obj) = manifest.as_object_mut() {
            // merge dependencies and devDependencies
            if let Some(optional_deps) = obj.get("optionalDependencies").and_then(|v| v.as_object())
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
        }

        log_verbose(&format!(
            "🔍 RegistryService::resolve_package completed for {name}@{spec} => {version}"
        ));

        Ok(ResolvedPackage {
            name: name.to_string(),
            version,
            manifest,
        })
    }
}

// Global resolve function
pub async fn resolve(name: &str, spec: &str) -> Result<ResolvedPackage> {
    let start_time = std::time::Instant::now();
    log_verbose(&format!("🔍 Starting resolve for {name}@{spec}"));

    let result = RegistryService::resolve_package(name, spec).await;

    match &result {
        Ok(resolved) => {
            log_verbose(&format!(
                "🔍 resolve completed for {}@{} => {} in {:?}",
                name,
                spec,
                resolved.version,
                start_time.elapsed()
            ));
        }
        Err(e) => {
            log_verbose(&format!(
                "🔍 resolve FAILED for {}@{} in {:?}: {}",
                name,
                spec,
                start_time.elapsed(),
                e
            ));
        }
    }

    result
}

pub async fn resolve_dependency(
    name: &str,
    spec: &str,
    edge_type: &EdgeType,
) -> Result<Option<ResolvedPackage>> {
    let start_time = std::time::Instant::now();
    log_verbose(&format!(
        "🔍 Starting resolve_dependency for {}@{} ({})",
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
                "🔍 resolve_dependency completed for {}@{} => {} in {:?}",
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
                    "🔍 resolve_dependency FAILED for {name}@{spec} after {elapsed:?}: {e}"
                ));
                Err(e)
            }
        }
    }
}
