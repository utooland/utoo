// chrono is used in from_package_info_and_manifest method
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Skip on error - try to deserialize, return None if fails
fn skip_on_error<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).ok())
}

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

/// Version-specific manifest from npm registry
/// This represents the JSON response from `npm view <package-name>@<version> --json`
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

    // Binary files configuration - can be string or object
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bin: Option<Value>,

    // Install script indicator (used by npm to optimize package installation)
    #[serde(rename = "hasInstallScript")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub has_install_script: Option<bool>,

    // Platform compatibility
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub cpu: Option<Value>, // Can be string or array

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub os: Option<Value>, // Can be string or array

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

impl VersionManifest {
    /// Create a VersionManifest from a package.json Value
    pub fn from_package_json(pkg: &Value) -> Result<Self, anyhow::Error> {
        // Create a basic VersionManifest from package.json data
        let manifest =
            Self {
                name: pkg
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                version: pkg
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.0.0")
                    .to_string(),
                description: pkg
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                main: pkg.get("main").and_then(|v| v.as_str()).map(String::from),
                scripts: pkg.get("scripts").and_then(|v| v.as_object()).map(|obj| {
                    obj.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                        .collect()
                }),
                repository: pkg
                    .get("repository")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                keywords: pkg.get("keywords").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                }),
                author: pkg
                    .get("author")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                license: pkg
                    .get("license")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                bugs: pkg
                    .get("bugs")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                homepage: pkg
                    .get("homepage")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                dependencies: pkg
                    .get("dependencies")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                            .collect()
                    }),
                dev_dependencies: pkg.get("devDependencies").and_then(|v| v.as_object()).map(
                    |obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                            .collect()
                    },
                ),
                peer_dependencies: pkg.get("peerDependencies").and_then(|v| v.as_object()).map(
                    |obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                            .collect()
                    },
                ),
                optional_dependencies: pkg
                    .get("optionalDependencies")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                            .collect()
                    }),
                bundled_dependencies: pkg
                    .get("bundledDependencies")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    }),
                engines: pkg.get("engines").and_then(|v| v.as_object()).map(|obj| {
                    obj.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("*").to_string()))
                        .collect()
                }),
                bin: pkg.get("bin").cloned(),
                has_install_script: pkg
                    .get("scripts")
                    .and_then(|v| v.as_object())
                    .map(|scripts| {
                        scripts.contains_key("preinstall")
                            || scripts.contains_key("install")
                            || scripts.contains_key("postinstall")
                    }),
                cpu: pkg.get("cpu").cloned(),
                os: pkg.get("os").cloned(),
                id: format!(
                    "{}@{}",
                    pkg.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    pkg.get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0.0.0")
                ),
                node_version: None,
                npm_version: None,
                dist: Dist::default(),
                npm_user: None,
                npm_operational_internal: None,
                directories: pkg
                    .get("directories")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
            };
        Ok(manifest)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Repository {
    #[serde(rename = "type")]
    pub repo_type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bugs {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Maintainer {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NpmUser {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NpmOperationalInternal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmp: Option<String>,
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionInfo {
    pub publish_time: Option<u64>,
    #[serde(rename = "_npmUser")]
    pub npm_user: Option<NpmUser>,
}

impl PackageManifest {
    pub fn from_package_info_and_manifest(
        package_info: &FullManifest,
        target_version_manifest: &VersionManifest,
    ) -> Self {
        // Get the latest version from dist-tags (currently unused, but kept for future use)
        let _latest_version = package_info.dist_tags.get("latest").map_or("latest", |v| v);

        // Use the target version
        let target_version = &target_version_manifest.version;

        // Extract versions info from time field and combine with npm user info
        let versions = if !package_info.time.is_empty() {
            let mut version_map = std::collections::HashMap::new();
            for version in package_info.time.keys() {
                if version == "created" || version == "modified" {
                    continue; // Skip meta timestamps
                }

                // Get npm user from the specific version manifest if available
                let npm_user = package_info
                    .versions
                    .get(version)
                    .and_then(|vm| vm.npm_user.as_ref())
                    .cloned();

                // Parse publish time from time field
                let publish_time = package_info.time.get(version).and_then(|time_str| {
                    // Try parsing ISO 8601 timestamp (e.g., "2021-05-20T22:47:07.137Z")
                    chrono::DateTime::parse_from_rfc3339(time_str)
                        .ok()
                        .map(|dt| dt.timestamp_millis() as u64)
                });

                let version_info = VersionInfo {
                    publish_time,
                    npm_user,
                };
                version_map.insert(version.clone(), version_info);
            }
            Some(version_map)
        } else {
            None
        };

        let versions_count = package_info.versions.len();

        // Extract dist-tags
        let dist_tags = if package_info.dist_tags.is_empty() {
            None
        } else {
            Some(package_info.dist_tags.clone())
        };

        PackageManifest {
            name: target_version_manifest.name.clone(),
            version: target_version.clone(),
            description: target_version_manifest.description.clone(),
            homepage: target_version_manifest.homepage.clone(),
            license: target_version_manifest.license.clone(),
            keywords: target_version_manifest.keywords.clone(),
            dependencies: target_version_manifest.dependencies.clone(),
            author: target_version_manifest
                .author
                .clone()
                .or_else(|| package_info.author.clone()),
            repository: target_version_manifest
                .repository
                .clone()
                .or_else(|| package_info.repository.clone()),
            bugs: target_version_manifest
                .bugs
                .clone()
                .or_else(|| package_info.bugs.clone()),
            dist: Some(target_version_manifest.dist.clone()),
            maintainers: package_info.maintainers.clone(),
            dist_tags,
            versions,
            versions_count,
        }
    }

    pub fn get_publish_time(&self, version: &str) -> Option<u64> {
        self.versions.as_ref()?.get(version)?.publish_time
    }

    pub fn get_npm_user(&self, version: &str) -> Option<&NpmUser> {
        self.versions.as_ref()?.get(version)?.npm_user.as_ref()
    }

    pub fn dependencies_count(&self) -> usize {
        self.dependencies
            .as_ref()
            .map(|deps| deps.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_author_string_deserialization() {
        let json = r#"{"author": "Erik Lieben <https://github.com/eriklieben>"}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub author: Option<Author>,
        }

        let manifest: TestManifest = serde_json::from_str(json).unwrap();

        // With skip_on_error, string author should fail to parse and return None
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
        assert_eq!(
            author.url,
            Some("https://github.com/eriklieben".to_string())
        );
    }

    #[test]
    fn test_author_null_deserialization() {
        let json = r#"{"author": null}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub author: Option<Author>,
        }

        let manifest: TestManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.author.is_none());
    }

    #[test]
    fn test_manifest_with_serde_default() {
        // Test that missing fields don't cause panic with #[serde(default)]
        let json = r#"{"name": "test-package"}"#;
        let manifest: FullManifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.name, "test-package");
        assert_eq!(manifest.description, None);
        assert!(manifest.dist_tags.is_empty());
        assert!(manifest.versions.is_empty());
    }

    #[test]
    fn test_keywords_flexible_deserialization() {
        // Test array keywords
        let json1 = r#"{"keywords": ["test", "package"]}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub keywords: Option<Vec<String>>,
        }

        let manifest1: TestManifest = serde_json::from_str(json1).unwrap();
        assert_eq!(
            manifest1.keywords,
            Some(vec!["test".to_string(), "package".to_string()])
        );

        // Test string keywords - should fail with skip_on_error
        let json2 = r#"{"keywords": "test"}"#;
        let manifest2: TestManifest = serde_json::from_str(json2).unwrap();
        assert_eq!(manifest2.keywords, None);
    }

    #[test]
    fn test_license_flexible_deserialization() {
        // Test string license
        let json1 = r#"{"license": "MIT"}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub license: Option<String>,
        }

        let manifest1: TestManifest = serde_json::from_str(json1).unwrap();
        assert_eq!(manifest1.license, Some("MIT".to_string()));

        // Test object license - should fail with skip_on_error
        let json2 = r#"{"license": {"type": "BSD-3-Clause"}}"#;
        let manifest2: TestManifest = serde_json::from_str(json2).unwrap();
        assert_eq!(manifest2.license, None);
    }

    #[test]
    fn test_bundled_deps_flexible_deserialization() {
        // Test array bundledDependencies
        let json1 = r#"{"bundledDependencies": ["dep1", "dep2"]}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(rename = "bundledDependencies", deserialize_with = "skip_on_error")]
            pub bundled_dependencies: Option<Vec<String>>,
        }

        let manifest1: TestManifest = serde_json::from_str(json1).unwrap();
        assert_eq!(
            manifest1.bundled_dependencies,
            Some(vec!["dep1".to_string(), "dep2".to_string()])
        );

        // Test object bundledDependencies - should fail with skip_on_error
        let json2 = r#"{"bundledDependencies": {"dep1": "1.0.0", "dep2": "2.0.0"}}"#;
        let manifest2: TestManifest = serde_json::from_str(json2).unwrap();
        assert!(manifest2.bundled_dependencies.is_none());
    }

    #[test]
    fn test_engines_flexible_deserialization() {
        // Test object engines (normal format)
        let json1 = r#"{"engines": {"node": ">=14.0.0", "npm": ">=6.0.0"}}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub engines: Option<HashMap<String, String>>,
        }

        let manifest1: TestManifest = serde_json::from_str(json1).unwrap();
        assert!(manifest1.engines.is_some());
        let engines = manifest1.engines.unwrap();
        assert_eq!(engines.get("node"), Some(&">=14.0.0".to_string()));
        assert_eq!(engines.get("npm"), Some(&">=6.0.0".to_string()));

        // Test array engines (jsonparse format) - should fail with skip_on_error
        let json2 = r#"{"engines": ["node >= 0.2.0"]}"#;
        let manifest2: TestManifest = serde_json::from_str(json2).unwrap();
        assert_eq!(manifest2.engines, None); // Should return None with skip_on_error
    }

    #[test]
    fn test_boolean_handling() {
        // Test boolean false as string field - should fail with skip_on_error
        let json1 = r#"{"license": false}"#;
        let manifest1: VersionManifest = serde_json::from_str(json1).unwrap();
        assert_eq!(manifest1.license, None);

        // Test boolean true as string field - should fail with skip_on_error
        let json2 = r#"{"license": true}"#;
        let manifest2: VersionManifest = serde_json::from_str(json2).unwrap();
        assert_eq!(manifest2.license, None);

        // Test boolean false as array field - should fail with skip_on_error
        let json3 = r#"{"keywords": false}"#;
        let manifest3: VersionManifest = serde_json::from_str(json3).unwrap();
        assert_eq!(manifest3.keywords, None);

        // Test boolean true as array field - should fail with skip_on_error
        let json4 = r#"{"keywords": true}"#;
        let manifest4: VersionManifest = serde_json::from_str(json4).unwrap();
        assert_eq!(manifest4.keywords, None);
    }


    #[test]
    fn test_jsonparse_real_manifest() {
        // Real jsonparse manifest data (simplified)
        let json = r#"{
            "name": "jsonparse",
            "description": "This is a pure-js JSON streaming parser for node.js",
            "version": "1.3.1",
            "author": { "name": "Tim Caswell", "email": "tim@creationix.com" },
            "repository": {
                "type": "git",
                "url": "git+ssh://git@github.com/creationix/jsonparse.git"
            },
            "devDependencies": { "tape": "~0.1.1", "tap": "~0.3.3" },
            "scripts": { "test": "tap test/*.js" },
            "bugs": { "url": "http://github.com/creationix/jsonparse/issues" },
            "engines": ["node >= 0.2.0"],
            "license": "MIT",
            "main": "jsonparse.js"
        }"#;

        let manifest: VersionManifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.name, "jsonparse");
        assert_eq!(manifest.version, "1.3.1");
        assert_eq!(manifest.license, Some("MIT".to_string()));

        // Author should be parsed correctly
        assert!(manifest.author.is_some());
        let author = manifest.author.unwrap();
        assert_eq!(author.name, "Tim Caswell");
        assert_eq!(author.email, Some("tim@creationix.com".to_string()));

        // Dependencies should be parsed correctly
        assert!(manifest.dev_dependencies.is_some());
        let dev_deps = manifest.dev_dependencies.unwrap();
        assert_eq!(dev_deps.get("tape"), Some(&"~0.1.1".to_string()));

        // Scripts should be parsed correctly
        assert!(manifest.scripts.is_some());
        let scripts = manifest.scripts.unwrap();
        assert_eq!(scripts.get("test"), Some(&"tap test/*.js".to_string()));

        // Engines should be None (array format not supported with skip_on_error)
        assert_eq!(manifest.engines, None);
    }
}
