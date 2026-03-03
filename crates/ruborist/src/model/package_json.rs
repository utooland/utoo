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

    /// Binary definitions (string or object)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<Value>,

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

    /// Package description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether package is private
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,

    /// Files whitelist for publishing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,

    /// Main entry point
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,

    /// TypeScript types entry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,

    /// TypeScript typings entry (legacy alias for types)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typings: Option<String>,

    /// Publish configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_config: Option<PublishConfig>,

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

/// Parse bin field from JSON Value.
/// Handles both string and object formats, filters out empty paths.
pub fn parse_bin_field(bin: &Value, package_name: &str) -> Vec<(String, String)> {
    bin.as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .or_else(|| {
            bin.as_str()
                .map(|s| vec![(package_name.to_string(), s.to_string())])
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, path)| !path.is_empty())
        .collect()
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

/// Publish configuration from package.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishConfig {
    /// Distribution tag (e.g., "latest", "beta")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// Registry URL override
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,

    /// Package access level ("public" or "restricted")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
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

    /// Get binary entries as (name, path) pairs.
    pub fn bin_entries(&self) -> Vec<(String, String)> {
        self.bin
            .as_ref()
            .map(|bin| parse_bin_field(bin, &self.name))
            .unwrap_or_default()
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
        assert_eq!(
            pkg.bin_entries(),
            vec![("my-cli".to_string(), "./cli.js".to_string())]
        );
    }

    #[test]
    fn test_parse_bin_map() {
        // Single key with custom name
        let value = json!({
            "name": "my-cli",
            "bin": {
                "a": "./index.js"
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(
            pkg.bin_entries(),
            vec![("a".to_string(), "./index.js".to_string())]
        );

        // Multiple keys
        let value = json!({
            "name": "my-tools",
            "bin": {
                "tool1": "./bin/tool1.js",
                "tool2": "./bin/tool2.js"
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin_entries();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|(k, v)| k == "tool1" && v == "./bin/tool1.js")
        );
        assert!(
            entries
                .iter()
                .any(|(k, v)| k == "tool2" && v == "./bin/tool2.js")
        );
    }

    #[test]
    fn test_parse_bin_empty_filtered() {
        // Empty bin string should be filtered out
        let value = json!({
            "name": "my-cli",
            "bin": ""
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.bin_entries().len(), 0);

        // Empty bin in map should be filtered out
        let value = json!({
            "name": "my-tools",
            "bin": {
                "tool1": "./bin/tool1.js",
                "empty": ""
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin_entries();
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
    fn test_parse_publish_fields() {
        let value = json!({
            "name": "my-pkg",
            "version": "1.0.0",
            "description": "A great package",
            "private": true,
            "files": ["dist", "lib"],
            "main": "./lib/index.js",
            "types": "./lib/index.d.ts",
            "typings": "./lib/index.d.ts",
            "publishConfig": {
                "tag": "beta",
                "registry": "https://custom.registry.org",
                "access": "public"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.description.as_deref(), Some("A great package"));
        assert_eq!(pkg.private, Some(true));
        assert_eq!(
            pkg.files.as_deref(),
            Some(&["dist".to_string(), "lib".to_string()][..])
        );
        assert_eq!(pkg.main.as_deref(), Some("./lib/index.js"));
        assert_eq!(pkg.types.as_deref(), Some("./lib/index.d.ts"));
        assert_eq!(pkg.typings.as_deref(), Some("./lib/index.d.ts"));

        let pc = pkg.publish_config.as_ref().unwrap();
        assert_eq!(pc.tag.as_deref(), Some("beta"));
        assert_eq!(pc.registry.as_deref(), Some("https://custom.registry.org"));
        assert_eq!(pc.access.as_deref(), Some("public"));

        // Round-trip: ensure new fields survive to_value
        let rt = pkg.to_value();
        assert_eq!(rt["description"], "A great package");
        assert_eq!(rt["private"], true);
        assert_eq!(rt["main"], "./lib/index.js");
        assert_eq!(rt["publishConfig"]["tag"], "beta");
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
