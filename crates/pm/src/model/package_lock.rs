use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::{
    helper::lock::path_to_pkg_name,
    service::dependency_graph::{DependencyGraphService, DependencyType, PackageNode},
};

/// Represents a license field that can be either a string or an array of strings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum License {
    String(String),
    Array(Vec<String>),
}

/// Represents package information in package-lock.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    pub license: Option<License>,
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
    #[serde(rename = "devOptional", skip_serializing_if = "Option::is_none")]
    pub dev_optional: Option<bool>,
    #[serde(rename = "hasInstallScript", skip_serializing_if = "Option::is_none")]
    pub has_install_script: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<Vec<String>>,
    // Additional fields from helper/lock.rs Package
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<serde_json::Value>,
}

impl LockPackage {
    /// Get package name, infer from path if not available
    pub fn get_name(&self, path: &str) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if path.is_empty() {
            "root".to_string()
        } else {
            // Use existing path_to_pkg_name helper function
            path_to_pkg_name(path)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        }
    }

    /// Get package version
    pub fn get_version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Parse and cache bin files from the bin field
    pub fn parse_bin_files(&self, package_name: &str) -> Vec<(String, String)> {
        match &self.bin {
            Some(serde_json::Value::Object(obj)) => obj
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect(),
            Some(serde_json::Value::String(s)) => vec![(package_name.to_string(), s.clone())],
            _ => Vec::new(),
        }
    }

    /// Check if package has install scripts
    pub fn has_install_scripts(&self) -> bool {
        self.has_install_script.unwrap_or(false)
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

        let package_lock: PackageLock =
            serde_json::from_str(&content).context("Failed to parse package-lock.json")?;

        Ok(package_lock)
    }

    /// Build dependency graph
    pub fn build_dependency_graph(&self) -> Result<DependencyGraphService> {
        let mut graph = DependencyGraphService::new();

        // First add all package nodes
        for (path, package) in &self.packages {
            let package_node = package.to_package_node(path);
            tracing::debug!("Adding package: {package_node:?}");
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
                        tracing::debug!(
                            "Warning: Failed to add production dependency {dep_name} for {from_package_name}: {e}"
                        );
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
                        tracing::debug!(
                            "Warning: Failed to add dev dependency {dep_name} for {from_package_name}: {e}"
                        );
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
                        tracing::debug!(
                            "Warning: Failed to add optional dependency {dep_name} for {from_package_name}: {e}"
                        );
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
                        tracing::debug!(
                            "Warning: Failed to add peer dependency {dep_name} for {from_package_name}: {e}"
                        );
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

    #[test]
    fn test_lock_package_parse_bin_files() {
        // Test with object-style bin
        let mut package = LockPackage {
            name: Some("test-package".to_string()),
            version: Some("1.0.0".to_string()),
            bin: Some(serde_json::json!({"cli": "bin/cli.js", "tool": "bin/tool.js"})),
            ..LockPackage::default()
        };

        let bin_files = package.parse_bin_files("test-package");
        assert_eq!(bin_files.len(), 2);
        assert!(bin_files.contains(&("cli".to_string(), "bin/cli.js".to_string())));
        assert!(bin_files.contains(&("tool".to_string(), "bin/tool.js".to_string())));

        // Test with string-style bin
        package.bin = Some(serde_json::json!("index.js"));
        let bin_files = package.parse_bin_files("test-package");
        assert_eq!(bin_files.len(), 1);
        assert_eq!(
            bin_files[0],
            ("test-package".to_string(), "index.js".to_string())
        );

        // Test with no bin
        package.bin = None;
        let bin_files = package.parse_bin_files("test-package");
        assert_eq!(bin_files.len(), 0);

        // Test with invalid bin value
        package.bin = Some(serde_json::json!(123));
        let bin_files = package.parse_bin_files("test-package");
        assert_eq!(bin_files.len(), 0);
    }

    #[test]
    fn test_lock_package_has_install_scripts() {
        let mut package = LockPackage {
            name: Some("test-package".to_string()),
            version: Some("1.0.0".to_string()),
            has_install_script: Some(true),
            ..LockPackage::default()
        };

        assert!(package.has_install_scripts());

        package.has_install_script = Some(false);
        assert!(!package.has_install_scripts());

        package.has_install_script = None;
        assert!(!package.has_install_scripts());
    }

    #[test]
    fn test_lock_package_get_name() {
        let mut package = LockPackage {
            name: Some("test-package".to_string()),
            version: Some("1.0.0".to_string()),
            ..LockPackage::default()
        };

        // Test with explicit name
        assert_eq!(
            package.get_name("node_modules/some-package"),
            "test-package"
        );

        // Test without name (infer from path)
        package.name = None;
        assert_eq!(package.get_name("node_modules/lodash"), "lodash");
        assert_eq!(
            package.get_name("node_modules/@scope/package"),
            "@scope/package"
        );

        // Test with empty path
        assert_eq!(package.get_name(""), "root");

        // Test with complex path
        assert_eq!(
            package.get_name("node_modules/parent/node_modules/child"),
            "child"
        );
    }

    #[test]
    fn test_lock_package_get_version() {
        let mut package = LockPackage {
            name: Some("test-package".to_string()),
            version: Some("1.2.3".to_string()),
            ..LockPackage::default()
        };

        assert_eq!(package.get_version(), "1.2.3");

        package.version = None;
        assert_eq!(package.get_version(), "unknown");
    }

    #[test]
    fn test_license_field_parsing() {
        // Test string license
        let json_string = r#"{
            "version": "1.0.0",
            "license": "MIT"
        }"#;
        let package: LockPackage = serde_json::from_str(json_string).unwrap();
        assert!(package.license.is_some());
        match package.license.unwrap() {
            License::String(s) => assert_eq!(s, "MIT"),
            License::Array(_) => panic!("Expected License::String"),
        }

        // Test array license
        let json_array = r#"{
            "version": "1.0.0",
            "license": ["MIT", "Apache-2.0"]
        }"#;
        let package: LockPackage = serde_json::from_str(json_array).unwrap();
        assert!(package.license.is_some());
        match package.license.unwrap() {
            License::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], "MIT");
                assert_eq!(arr[1], "Apache-2.0");
            }
            License::String(_) => panic!("Expected License::Array"),
        }

        // Test no license
        let json_no_license = r#"{
            "version": "1.0.0"
        }"#;
        let package: LockPackage = serde_json::from_str(json_no_license).unwrap();
        assert!(package.license.is_none());

        // Test serialization of string license
        let package_with_string = LockPackage {
            version: Some("1.0.0".to_string()),
            license: Some(License::String("MIT".to_string())),
            ..LockPackage::default()
        };
        let serialized = serde_json::to_string(&package_with_string).unwrap();
        assert!(serialized.contains(r#""license":"MIT""#));

        // Test serialization of array license
        let package_with_array = LockPackage {
            version: Some("1.0.0".to_string()),
            license: Some(License::Array(vec![
                "MIT".to_string(),
                "Apache-2.0".to_string(),
            ])),
            ..LockPackage::default()
        };
        let serialized = serde_json::to_string(&package_with_array).unwrap();
        assert!(serialized.contains(r#""license":["MIT","Apache-2.0"]"#));
    }
}
