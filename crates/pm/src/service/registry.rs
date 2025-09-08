use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::http_client::{get_package_manifest, get_package_manifest_with_semver};
use crate::model::node::EdgeType;
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
        // Try the new semver-based approach first
        let (version, mut manifest) = match get_package_manifest_with_semver(name, spec).await {
            Ok(result) => result,
            Err(_) => {
                // Fallback to original method for compatibility
                log_verbose(&format!("Falling back to original method for {name}@{spec}"));
                get_package_manifest(name, spec).await?
            }
        };

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

        Ok(ResolvedPackage {
            name: name.to_string(),
            version,
            manifest,
        })
    }
}

// Global resolve function
pub async fn resolve(name: &str, spec: &str) -> Result<ResolvedPackage> {
    RegistryService::resolve_package(name, spec).await
}

pub async fn resolve_dependency(
    name: &str,
    spec: &str,
    edge_type: &EdgeType,
) -> Result<Option<ResolvedPackage>> {
    match resolve(name, spec).await {
        Ok(resolved) => Ok(Some(resolved)),
        Err(e) => {
            if *edge_type == EdgeType::Optional {
                log_verbose(&format!(
                    "skipping optional dependency {name}@{spec} due to resolve error: {e}"
                ));
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}
