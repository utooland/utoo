use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub dependencies: Option<HashMap<String, String>>,
    pub author: Option<Author>,
    pub repository: Option<Repository>,
    pub bugs: Option<Bugs>,
    pub dist: Option<Dist>,
    pub maintainers: Option<Vec<Maintainer>>,
    pub dist_tags: Option<HashMap<String, String>>,
    pub versions: Option<HashMap<String, VersionInfo>>,
    pub versions_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    #[serde(rename = "type")]
    pub repo_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bugs {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dist {
    pub tarball: Option<String>,
    pub shasum: Option<String>,
    pub integrity: Option<String>,
    #[serde(rename = "unpackedSize")]
    pub unpacked_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Maintainer {
    pub name: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub publish_time: Option<u64>,
    #[serde(rename = "_npmUser")]
    pub npm_user: Option<NpmUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmUser {
    pub name: String,
    pub email: Option<String>,
}

impl PackageManifest {
    pub fn from_package_info_and_manifest(
        package_info: &Value,
        name: &str,
        version_manifest: &Value,
    ) -> Self {
        // Get the latest version from package info
        let latest_version = package_info
            .get("dist-tags")
            .and_then(|tags| tags.get("latest"))
            .and_then(|v| v.as_str())
            .unwrap_or("latest");

        // Get the specific version if provided, otherwise use latest
        let target_version = version_manifest
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or(latest_version);

        // Extract dependencies
        let dependencies = version_manifest
            .get("dependencies")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            });

        // Extract author
        let author_source = version_manifest
            .get("author")
            .or_else(|| package_info.get("author"));
        let author = author_source.and_then(|v| v.as_object()).map(|obj| Author {
            name: obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            email: obj
                .get("email")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });

        // Extract repository
        let repo_source = version_manifest
            .get("repository")
            .or_else(|| package_info.get("repository"));
        let repository = repo_source.and_then(|v| v.as_object()).and_then(|obj| {
            let repo_type = obj.get("type")?.as_str()?;
            let url = obj.get("url")?.as_str()?;
            Some(Repository {
                repo_type: repo_type.to_string(),
                url: url.to_string(),
            })
        });

        // Extract bugs
        let bugs_source = version_manifest
            .get("bugs")
            .or_else(|| package_info.get("bugs"));
        let bugs = bugs_source
            .and_then(|v| v.as_object())
            .and_then(|obj| obj.get("url")?.as_str())
            .map(|url| Bugs {
                url: url.to_string(),
            });

        // Extract dist
        let dist = version_manifest
            .get("dist")
            .and_then(|v| v.as_object())
            .map(|obj| Dist {
                tarball: obj
                    .get("tarball")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                shasum: obj
                    .get("shasum")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                integrity: obj
                    .get("integrity")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                unpacked_size: obj.get("unpackedSize").and_then(|v| v.as_u64()),
            });

        // Extract maintainers
        let maintainers = package_info
            .get("maintainers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|maintainer| {
                        maintainer.as_object().and_then(|obj| {
                            let name = obj.get("name")?.as_str()?.to_string();
                            let email = obj
                                .get("email")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            Some(Maintainer { name, email })
                        })
                    })
                    .collect()
            });

        // Extract dist-tags
        let dist_tags = package_info
            .get("dist-tags")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            });

        // Extract versions info
        let versions = package_info
            .get("versions")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| {
                        let version_info = VersionInfo {
                            publish_time: v.get("publish_time").and_then(|pt| pt.as_u64()),
                            npm_user: v.get("_npmUser").and_then(|user| {
                                user.as_object().and_then(|obj| {
                                    let name = obj.get("name")?.as_str()?.to_string();
                                    let email = obj
                                        .get("email")
                                        .and_then(|e| e.as_str())
                                        .map(|s| s.to_string());
                                    Some(NpmUser { name, email })
                                })
                            }),
                        };
                        (k.clone(), version_info)
                    })
                    .collect()
            });

        let versions_count = package_info
            .get("versions")
            .and_then(|v| v.as_object())
            .map(|obj| obj.len())
            .unwrap_or(0);

        // Extract keywords
        let keywords = version_manifest
            .get("keywords")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|k| k.as_str())
                    .map(|s| s.to_string())
                    .collect()
            });

        // Get license from multiple sources
        let license = version_manifest
            .get("license")
            .and_then(|v| v.as_str())
            .or_else(|| package_info.get("license").and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        PackageManifest {
            name: name.to_string(),
            version: target_version.to_string(),
            description: version_manifest
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            homepage: version_manifest
                .get("homepage")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            license,
            keywords,
            dependencies,
            author,
            repository,
            bugs,
            dist,
            maintainers,
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
