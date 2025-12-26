//! Package.json model type for ruborist.
//!
//! This module provides strongly-typed representations of package.json content,
//! avoiding the need to pass raw `serde_json::Value` through the API.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Parsed package.json content.
///
/// This is the primary type for representing package.json in ruborist.
/// It contains all fields needed for dependency resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    /// Package name
    #[serde(default)]
    pub name: String,

    /// Package version
    #[serde(default)]
    pub version: String,

    /// Production dependencies
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub dependencies: HashMap<String, String>,

    /// Development dependencies
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub dev_dependencies: HashMap<String, String>,

    /// Peer dependencies
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub peer_dependencies: HashMap<String, String>,

    /// Optional dependencies
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub optional_dependencies: HashMap<String, String>,

    /// Overrides (npm) / resolutions (yarn)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Value>,

    /// Workspaces configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<WorkspacesConfig>,

    /// Engine requirements
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engines: Option<HashMap<String, String>>,

    /// Binary definitions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinConfig>,

    /// Package license
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<LicenseConfig>,

    /// Scripts
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub scripts: HashMap<String, String>,

    /// OS constraints
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<String>>,

    /// CPU constraints
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Vec<String>>,

    /// Has install scripts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_install_script: Option<bool>,

    /// Distribution info (from registry)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dist: Option<DistInfo>,

    /// Preserve other fields we don't explicitly model
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Workspaces configuration (can be array or object with packages field).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspacesConfig {
    /// Simple array of glob patterns
    Array(Vec<String>),
    /// Object with packages field
    Object { packages: Vec<String> },
}

impl WorkspacesConfig {
    /// Get the workspace patterns.
    pub fn patterns(&self) -> &[String] {
        match self {
            WorkspacesConfig::Array(patterns) => patterns,
            WorkspacesConfig::Object { packages } => packages,
        }
    }
}

/// Binary configuration (can be string or map).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinConfig {
    /// Single binary with package name
    Single(String),
    /// Multiple binaries
    Map(HashMap<String, String>),
}

impl BinConfig {
    /// Get all binary entries as (name, path) pairs.
    /// Empty bin paths are filtered out.
    pub fn entries(&self, package_name: &str) -> Vec<(String, String)> {
        let entries: Vec<_> = match self {
            BinConfig::Single(path) => vec![(package_name.to_string(), path.clone())],
            BinConfig::Map(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };
        entries.into_iter().filter(|(_, p)| !p.is_empty()).collect()
    }
}

/// License configuration (can be string or object).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LicenseConfig {
    /// SPDX license identifier
    String(String),
    /// Object with type and url
    Object { r#type: String, url: Option<String> },
}

impl LicenseConfig {
    /// Get the license identifier.
    pub fn identifier(&self) -> &str {
        match self {
            LicenseConfig::String(s) => s,
            LicenseConfig::Object { r#type, .. } => r#type,
        }
    }
}

/// Distribution information from registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DistInfo {
    /// Tarball URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tarball: Option<String>,

    /// Integrity hash (SRI format)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,

    /// SHA-1 hash (legacy)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,

    /// File count
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,

    /// Unpacked size
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpacked_size: Option<u64>,
}

impl PackageJson {
    /// Create a new empty PackageJson.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    /// Parse from JSON Value.
    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value.clone())
    }

    /// Convert to JSON Value.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Check if package has any dependencies.
    pub fn has_dependencies(&self) -> bool {
        !self.dependencies.is_empty()
            || !self.dev_dependencies.is_empty()
            || !self.peer_dependencies.is_empty()
            || !self.optional_dependencies.is_empty()
    }

    /// Get all dependency types as iterator.
    pub fn all_dependencies(&self) -> impl Iterator<Item = (&str, &HashMap<String, String>)> {
        [
            ("dependencies", &self.dependencies),
            ("devDependencies", &self.dev_dependencies),
            ("peerDependencies", &self.peer_dependencies),
            ("optionalDependencies", &self.optional_dependencies),
        ]
        .into_iter()
        .filter(|(_, deps)| !deps.is_empty())
    }

    /// Get tarball URL from dist info.
    pub fn tarball_url(&self) -> Option<&str> {
        self.dist.as_ref().and_then(|d| d.tarball.as_deref())
    }

    /// Get integrity hash from dist info.
    pub fn integrity(&self) -> Option<&str> {
        self.dist.as_ref().and_then(|d| d.integrity.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_simple_package() {
        let value = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.0"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.name, "test-package");
        assert_eq!(pkg.version, "1.0.0");
        assert_eq!(pkg.dependencies.get("lodash"), Some(&"^4.17.0".to_string()));
    }

    #[test]
    fn test_parse_workspaces_array() {
        let value = json!({
            "name": "monorepo",
            "workspaces": ["packages/*"]
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let workspaces = pkg.workspaces.unwrap();
        let patterns = workspaces.patterns();
        assert_eq!(patterns, &["packages/*"]);
    }

    #[test]
    fn test_parse_workspaces_object() {
        let value = json!({
            "name": "monorepo",
            "workspaces": {
                "packages": ["packages/*", "apps/*"]
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let workspaces = pkg.workspaces.unwrap();
        let patterns = workspaces.patterns();
        assert_eq!(patterns, &["packages/*", "apps/*"]);
    }

    #[test]
    fn test_parse_bin_string() {
        let value = json!({
            "name": "my-cli",
            "bin": "./cli.js"
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin.unwrap().entries("my-cli");
        assert_eq!(
            entries,
            vec![("my-cli".to_string(), "./cli.js".to_string())]
        );
    }

    #[test]
    fn test_parse_bin_map() {
        let value = json!({
            "name": "my-tools",
            "bin": {
                "tool1": "./bin/tool1.js",
                "tool2": "./bin/tool2.js"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin.unwrap().entries("my-tools");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_bin_empty_filtered() {
        // Empty bin string should be filtered out
        let value = json!({
            "name": "my-cli",
            "bin": ""
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin.unwrap().entries("my-cli");
        assert_eq!(entries.len(), 0);

        // Empty bin in map should be filtered out
        let value = json!({
            "name": "my-tools",
            "bin": {
                "tool1": "./bin/tool1.js",
                "empty": ""
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin.unwrap().entries("my-tools");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "tool1");
    }

    #[test]
    fn test_parse_dist_info() {
        let value = json!({
            "name": "lodash",
            "version": "4.17.21",
            "dist": {
                "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
                "integrity": "sha512-xyz"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(
            pkg.tarball_url(),
            Some("https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz")
        );
        assert_eq!(pkg.integrity(), Some("sha512-xyz"));
    }

    #[test]
    fn test_roundtrip() {
        let original = json!({
            "name": "test",
            "version": "1.0.0",
            "dependencies": { "lodash": "^4.0.0" },
            "devDependencies": { "jest": "^29.0.0" },
            "scripts": { "test": "jest" }
        });

        let pkg = PackageJson::from_value(&original).unwrap();
        let value = pkg.to_value();

        assert_eq!(value["name"], "test");
        assert_eq!(value["version"], "1.0.0");
        assert_eq!(value["dependencies"]["lodash"], "^4.0.0");
    }
}
