//! npm registry manifest types.
//!
//! These types represent the JSON responses from npm registry API.
//! Used by both PM (native) and WASM (browser) implementations.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Skip on error - try to deserialize, return None if fails.
/// This handles malformed npm registry data gracefully.
fn skip_on_error<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).ok())
}

/// Full package manifest from npm registry.
/// This is the response from `GET /<package-name>` endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FullManifest {
    #[serde(rename = "_id")]
    pub id: Option<String>,

    #[serde(rename = "_rev")]
    pub rev: Option<String>,

    pub name: String,

    pub description: Option<String>,

    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,

    #[serde(default)]
    pub versions: HashMap<String, VersionManifest>,

    pub time: HashMap<String, String>,

    #[serde(deserialize_with = "skip_on_error")]
    pub maintainers: Option<Vec<Maintainer>>,

    #[serde(deserialize_with = "skip_on_error")]
    pub author: Option<Author>,

    #[serde(deserialize_with = "skip_on_error")]
    pub repository: Option<Repository>,

    #[serde(deserialize_with = "skip_on_error")]
    pub bugs: Option<Bugs>,

    #[serde(deserialize_with = "skip_on_error")]
    pub homepage: Option<String>,

    #[serde(deserialize_with = "skip_on_error")]
    pub keywords: Option<Vec<String>>,

    #[serde(deserialize_with = "skip_on_error")]
    pub license: Option<String>,

    #[serde(deserialize_with = "skip_on_error")]
    pub readme: Option<String>,

    #[serde(rename = "readmeFilename")]
    #[serde(deserialize_with = "skip_on_error")]
    pub readme_filename: Option<String>,
}

/// Version-specific manifest from npm registry.
/// This represents a single version entry in the `versions` field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionManifest {
    pub name: String,
    pub version: String,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub main: Option<String>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub scripts: Option<HashMap<String, String>>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub repository: Option<Repository>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub keywords: Option<Vec<String>>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<Author>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub license: Option<String>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bugs: Option<Bugs>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub homepage: Option<String>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dependencies: Option<HashMap<String, String>>,

    #[serde(rename = "devDependencies")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dev_dependencies: Option<HashMap<String, String>>,

    #[serde(rename = "peerDependencies")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_dependencies: Option<HashMap<String, String>>,

    #[serde(rename = "optionalDependencies")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional_dependencies: Option<HashMap<String, String>>,

    #[serde(
        rename = "bundledDependencies",
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bundled_dependencies: Option<Vec<String>>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub engines: Option<HashMap<String, String>>,

    /// Binary files configuration - can be string or object
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bin: Option<Value>,

    /// Install script indicator (used by npm to optimize package installation)
    #[serde(rename = "hasInstallScript")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub has_install_script: Option<bool>,

    /// Platform compatibility - CPU
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub cpu: Option<Value>,

    /// Platform compatibility - OS
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub os: Option<Value>,

    #[serde(rename = "_id")]
    pub id: String,

    #[serde(rename = "_nodeVersion")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub node_version: Option<String>,

    #[serde(rename = "_npmVersion")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub npm_version: Option<String>,

    pub dist: Dist,

    #[serde(rename = "_npmUser")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub npm_user: Option<NpmUser>,

    #[serde(rename = "_npmOperationalInternal")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_operational_internal: Option<NpmOperationalInternal>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub directories: Option<Directories>,
}

/// Package author information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Repository information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Repository {
    #[serde(rename = "type")]
    pub repo_type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

/// Bug tracker information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bugs {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Distribution information for a package version.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dist {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tarball: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,

    #[serde(rename = "fileCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,

    #[serde(rename = "unpackedSize")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked_size: Option<u64>,

    #[serde(rename = "npm-signature")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_signature: Option<String>,
}

/// Package maintainer information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Maintainer {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// npm user information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NpmUser {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// npm operational internal metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NpmOperationalInternal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmp: Option<String>,
}

/// Directory paths in package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Directories {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub man: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
}

/// Simplified package manifest (for `npm view` output).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub homepage: Option<String>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub license: Option<String>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub keywords: Option<Vec<String>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<Author>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub repository: Option<Repository>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bugs: Option<Bugs>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dist: Option<Dist>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub maintainers: Option<Vec<Maintainer>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dist_tags: Option<HashMap<String, String>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub versions: Option<HashMap<String, VersionInfo>>,
    pub versions_count: usize,
}

/// Simplified version info.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct VersionInfo {
    pub publish_time: Option<u64>,
    #[serde(rename = "_npmUser")]
    pub npm_user: Option<NpmUser>,
}

use super::package_json::PackageJson;

/// Manifest for a node in the dependency graph.
///
/// This enum distinguishes between local packages (root/workspace) and
/// registry packages (resolved dependencies).
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum NodeManifest {
    /// Local package.json (root or workspace)
    Local(PackageJson),
    /// Registry package manifest (resolved dependency)
    Registry(VersionManifest),
}

impl NodeManifest {
    /// Get the package name.
    pub fn name(&self) -> &str {
        match self {
            NodeManifest::Local(pkg) => &pkg.name,
            NodeManifest::Registry(manifest) => &manifest.name,
        }
    }

    /// Get the package version.
    pub fn version(&self) -> &str {
        match self {
            NodeManifest::Local(pkg) => &pkg.version,
            NodeManifest::Registry(manifest) => &manifest.version,
        }
    }

    /// Get production dependencies.
    pub fn dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => {
                if pkg.dependencies.is_empty() {
                    None
                } else {
                    Some(&pkg.dependencies)
                }
            }
            NodeManifest::Registry(manifest) => manifest.dependencies.as_ref(),
        }
    }

    /// Get peer dependencies.
    pub fn peer_dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => {
                if pkg.peer_dependencies.is_empty() {
                    None
                } else {
                    Some(&pkg.peer_dependencies)
                }
            }
            NodeManifest::Registry(manifest) => manifest.peer_dependencies.as_ref(),
        }
    }

    /// Get optional dependencies.
    pub fn optional_dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => {
                if pkg.optional_dependencies.is_empty() {
                    None
                } else {
                    Some(&pkg.optional_dependencies)
                }
            }
            NodeManifest::Registry(manifest) => manifest.optional_dependencies.as_ref(),
        }
    }

    /// Get dev dependencies (only for local packages).
    pub fn dev_dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => {
                if pkg.dev_dependencies.is_empty() {
                    None
                } else {
                    Some(&pkg.dev_dependencies)
                }
            }
            NodeManifest::Registry(_) => None, // Registry packages don't include devDeps
        }
    }

    /// Get engines requirements.
    pub fn engines(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.engines.as_ref(),
            NodeManifest::Registry(manifest) => manifest.engines.as_ref(),
        }
    }

    /// Get binary configuration as Value (for serialization compatibility).
    pub fn bin(&self) -> Option<Value> {
        match self {
            NodeManifest::Local(pkg) => pkg.bin.as_ref().and_then(|b| serde_json::to_value(b).ok()),
            NodeManifest::Registry(manifest) => manifest.bin.clone(),
        }
    }

    /// Get license.
    pub fn license(&self) -> Option<String> {
        match self {
            NodeManifest::Local(pkg) => pkg.license.as_ref().map(|l| l.identifier().to_string()),
            NodeManifest::Registry(manifest) => manifest.license.clone(),
        }
    }

    /// Get OS constraints.
    pub fn os(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(_) => None, // PackageJson uses Vec<String>
            NodeManifest::Registry(manifest) => manifest.os.as_ref(),
        }
    }

    /// Get CPU constraints.
    pub fn cpu(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(_) => None, // PackageJson uses Vec<String>
            NodeManifest::Registry(manifest) => manifest.cpu.as_ref(),
        }
    }

    /// Check if has install script.
    pub fn has_install_script(&self) -> bool {
        match self {
            NodeManifest::Local(pkg) => pkg.has_install_script.unwrap_or(false),
            NodeManifest::Registry(manifest) => manifest.has_install_script.unwrap_or(false),
        }
    }

    /// Get scripts.
    pub fn scripts(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => {
                if pkg.scripts.is_empty() {
                    None
                } else {
                    Some(&pkg.scripts)
                }
            }
            NodeManifest::Registry(manifest) => manifest.scripts.as_ref(),
        }
    }

    /// Get distribution info (tarball, integrity).
    pub fn dist(&self) -> Option<&Dist> {
        match self {
            NodeManifest::Local(_) => None,
            NodeManifest::Registry(manifest) => Some(&manifest.dist),
        }
    }

    /// Get workspaces configuration (only for local packages).
    pub fn workspaces(&self) -> Option<Value> {
        match self {
            NodeManifest::Local(pkg) => pkg
                .workspaces
                .as_ref()
                .and_then(|w| serde_json::to_value(w).ok()),
            NodeManifest::Registry(_) => None,
        }
    }

    /// Get overrides configuration (only for local packages).
    pub fn overrides(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(pkg) => pkg.overrides.as_ref(),
            NodeManifest::Registry(_) => None,
        }
    }
}

impl From<PackageJson> for NodeManifest {
    fn from(pkg: PackageJson) -> Self {
        NodeManifest::Local(pkg)
    }
}

impl From<VersionManifest> for NodeManifest {
    fn from(manifest: VersionManifest) -> Self {
        NodeManifest::Registry(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_author_string_deserialization() {
        let json = r#"{"author": "Erik Lieben <https://github.com/eriklieben>"}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub author: Option<Author>,
        }

        let manifest: TestManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.author.is_none());
    }

    #[test]
    fn test_author_object_deserialization() {
        let json = r#"{"author": {"name": "Erik Lieben", "email": "erik@example.com", "url": "https://github.com/eriklieben"}}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub author: Option<Author>,
        }

        let manifest: TestManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.author.is_some());
        let author = manifest.author.unwrap();
        assert_eq!(author.name, "Erik Lieben");
        assert_eq!(author.email, Some("erik@example.com".to_string()));
    }

    #[test]
    fn test_manifest_with_serde_default() {
        let json = r#"{"name": "test-package"}"#;
        let manifest: FullManifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.name, "test-package");
        assert_eq!(manifest.description, None);
        assert!(manifest.dist_tags.is_empty());
        assert!(manifest.versions.is_empty());
    }

    #[test]
    fn test_version_manifest_parsing() {
        let json = r#"{
            "name": "jsonparse",
            "version": "1.3.1",
            "license": "MIT",
            "dependencies": { "lodash": "^4.0.0" }
        }"#;

        let manifest: VersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "jsonparse");
        assert_eq!(manifest.version, "1.3.1");
        assert_eq!(manifest.license, Some("MIT".to_string()));
        assert!(manifest.dependencies.is_some());
    }
}
