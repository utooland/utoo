use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents package information in package-lock.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockPackage {
    pub name: Option<String>,
    pub version: Option<String>,
    pub resolved: Option<String>,
    pub integrity: Option<String>,
    pub license: Option<String>,
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "peerDependencies")]
    pub peer_dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "optionalDependencies")]
    pub optional_dependencies: Option<HashMap<String, String>>,
    pub requires: Option<HashMap<String, String>>,
    pub bin: Option<serde_json::Value>,
    pub peer: Option<bool>,
    pub dev: Option<bool>,
    pub optional: Option<bool>,
    #[serde(rename = "hasInstallScript")]
    pub has_install_script: Option<bool>,
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
            path.rsplit('/').next().unwrap_or("unknown").to_string()
        }
    }
    /// Get package version
    pub fn get_version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
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

    pub dependencies: Option<HashMap<String, serde_json::Value>>,
}

impl PackageLock {
    /// Parse from json string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
