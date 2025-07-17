use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::{
    service::dependency_graph::{DependencyGraphService, DependencyType, PackageNode},
    util::logger::log_verbose,
};

/// Represents package information in package-lock.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockPackage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies", skip_serializing_if = "Option::is_none")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies", skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(
        rename = "optionalDependencies",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional_dependencies: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(rename = "hasInstallScript", skip_serializing_if = "Option::is_none")]
    pub has_install_script: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,
}

impl LockPackage {
    /// Get package name, infer from path if not available
    pub fn get_name(&self, path: &str) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if path.is_empty() {
            "root".to_string()
        } else {
            // Extract package name from path
            path.split('/').next_back().unwrap_or("unknown").to_string()
        }
    }

    /// Get package version
    pub fn get_version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Convert to PackageNode
    pub fn to_package_node(&self, path: &str) -> PackageNode {
        let name = self.get_name(path);
        let version = self.get_version();

        let mut package_node = PackageNode::new(name, version, path.to_string());

        if let Some(deps) = &self.dependencies {
            package_node.dependencies = deps.clone();
        }

        if let Some(dev_deps) = &self.dev_dependencies {
            package_node.dev_dependencies = dev_deps.clone();
        }

        if let Some(peer_deps) = &self.peer_dependencies {
            package_node.peer_dependencies = peer_deps.clone();
        }

        if let Some(opt_deps) = &self.optional_dependencies {
            package_node.optional_dependencies = opt_deps.clone();
        }

        package_node
    }
}

/// Represents complete package-lock.json file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageLock {
    pub name: String,
    pub version: String,
    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: u32,
    pub requires: bool,
    pub packages: HashMap<String, LockPackage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, serde_json::Value>>,
}

impl PackageLock {
    /// Load from package-lock.json file
    pub fn from_lock_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content =
            fs::read_to_string(path.as_ref()).context("Failed to read package-lock.json file")?;

        let package_lock: PackageLock = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse package-lock.json: {}", e))?;

        Ok(package_lock)
    }

    /// Build dependency graph
    pub fn build_dependency_graph(&self) -> Result<DependencyGraphService> {
        let mut graph = DependencyGraphService::new();

        // First add all package nodes
        for (path, package) in &self.packages {
            let package_node = package.to_package_node(path);
            log_verbose(&format!("Adding package: {package_node:?}"));
            graph.add_package(package_node)?;
        }

        // Then add dependency relationships
        for (path, package) in &self.packages {
            let package_node = package.to_package_node(path);
            let from_package_name = package_node.name.clone();

            // Add production dependencies
            if let Some(deps) = &package.dependencies {
                for (dep_name, dep_version) in deps {
                    // Use path resolution logic to find correct dependency package
                    if let Err(e) = graph.add_dependency_with_path(
                        path,
                        &from_package_name,
                        dep_name,
                        DependencyType::Production,
                        dep_version.clone(),
                    ) {
                        log_verbose(&format!(
                            "Warning: Failed to add production dependency {dep_name} for {from_package_name}: {e}"
                        ));
                    }
                }
            }

            // Add development dependencies
            if let Some(dev_deps) = &package.dev_dependencies {
                for (dep_name, dep_version) in dev_deps {
                    if let Err(e) = graph.add_dependency_with_path(
                        path,
                        &from_package_name,
                        dep_name,
                        DependencyType::Development,
                        dep_version.clone(),
                    ) {
                        log_verbose(&format!(
                            "Warning: Failed to add dev dependency {dep_name} for {from_package_name}: {e}"
                        ));
                    }
                }
            }

            // Add optional dependencies
            if let Some(opt_deps) = &package.optional_dependencies {
                for (dep_name, dep_version) in opt_deps {
                    if let Err(e) = graph.add_dependency_with_path(
                        path,
                        &from_package_name,
                        dep_name,
                        DependencyType::Optional,
                        dep_version.clone(),
                    ) {
                        log_verbose(&format!(
                            "Warning: Failed to add optional dependency {dep_name} for {from_package_name}: {e}"
                        ));
                    }
                }
            }

            // Add peer dependencies
            if let Some(peer_deps) = &package.peer_dependencies {
                for (dep_name, dep_version) in peer_deps {
                    if let Err(e) = graph.add_dependency_with_path(
                        path,
                        &from_package_name,
                        dep_name,
                        DependencyType::Peer,
                        dep_version.clone(),
                    ) {
                        log_verbose(&format!(
                            "Warning: Failed to add peer dependency {dep_name} for {from_package_name}: {e}"
                        ));
                    }
                }
            }
        }

        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_lock_parsing() {
        let lock_json = r#"
        {
            "name": "test-project",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-project",
                    "version": "1.0.0",
                    "dependencies": {
                        "lodash": "^4.17.21"
                    },
                    "devDependencies": {
                        "typescript": "^5.0.0"
                    }
                },
                "node_modules/lodash": {
                    "version": "4.17.21",
                    "license": "MIT"
                },
                "node_modules/typescript": {
                    "version": "5.0.0",
                    "dev": true,
                    "license": "Apache-2.0"
                }
            }
        }"#;

        let package_lock: PackageLock = serde_json::from_str(lock_json).unwrap();

        assert_eq!(package_lock.name, "test-project");
        assert_eq!(package_lock.version, "1.0.0");
        assert_eq!(package_lock.lockfile_version, 3);
        assert_eq!(package_lock.packages.len(), 3);
    }
}
