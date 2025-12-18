//! Package-lock.json data structures.
//!
//! Shared types for serializing/deserializing npm lock files.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a license field that can be either a string or an array of strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum License {
    String(String),
    Array(Vec<String>),
}

/// Represents package information in package-lock.json.
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
    pub bin: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripts: Option<serde_json::Value>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<bool>,
}

impl LockPackage {
    /// Extract package name from path string.
    /// Handles both normal and scoped packages.
    pub fn path_to_pkg_name(path_str: &str) -> Option<&str> {
        if let Some(idx) = path_str.rfind("node_modules/") {
            let pkg_name = &path_str[idx + "node_modules/".len()..];
            let parts: Vec<&str> = pkg_name.split('/').collect();
            // Only allow pkg or @scope/pkg, skip deep paths
            if parts.len() > 2 || (parts.len() == 2 && !parts[0].starts_with('@')) {
                return None;
            }
            Some(pkg_name)
        } else {
            None
        }
    }

    /// Get package name, infer from path if not available.
    pub fn get_name(&self, path: &str) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if path.is_empty() {
            "root".to_string()
        } else {
            Self::path_to_pkg_name(path)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        }
    }

    /// Get package version.
    pub fn get_version(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Parse bin files from the bin field.
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

    /// Check if package has install scripts.
    pub fn has_install_scripts(&self) -> bool {
        self.has_install_script.unwrap_or(false)
    }
}

/// A wrapper around LockPackage that includes the path.
/// Used for querying dependency graph from package-lock.json.
#[derive(Debug, Clone)]
pub struct LockPackageNode {
    /// Path in package-lock.json (e.g., "node_modules/lodash")
    pub path: String,
    /// The underlying LockPackage data
    pub package: LockPackage,
}

impl LockPackageNode {
    pub fn new(path: String, package: LockPackage) -> Self {
        Self { path, package }
    }

    /// Get package name
    pub fn name(&self) -> String {
        self.package.get_name(&self.path)
    }

    /// Get package version
    pub fn version(&self) -> String {
        self.package.get_version()
    }
}

/// Represents complete package-lock.json file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageLock {
    pub name: String,
    pub version: String,
    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: u32,
    #[serde(default)]
    pub requires: bool,
    pub packages: HashMap<String, LockPackage>,
}

impl PackageLock {
    /// Create a new PackageLock with default values.
    pub fn new(name: String, version: String, packages: HashMap<String, LockPackage>) -> Self {
        Self {
            name,
            version,
            lockfile_version: 3,
            requires: true,
            packages,
        }
    }

    /// Convert from graph's serialized packages Value to PackageLock.
    pub fn from_packages_value(
        name: String,
        version: String,
        packages: serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        let packages_map: HashMap<String, LockPackage> = serde_json::from_value(packages)?;
        Ok(Self::new(name, version, packages_map))
    }
}

// ============================================================================
// Graph Serialization
// ============================================================================

use std::path::Path;

use petgraph::graph::NodeIndex;
use serde_json::{Value, json};

use super::graph::DependencyGraph;
use super::node::EdgeType;

/// Serialize a dependency graph to PackageLock format.
///
/// This is the main entry point for converting a resolved dependency graph
/// into a package-lock.json compatible structure.
pub fn serialize_graph(graph: &DependencyGraph, root_path: &Path) -> PackageLock {
    let (packages_value, _total) = serialize_to_packages(graph, root_path);

    let root_node = graph
        .get_node(graph.root_index)
        .expect("Graph must have a root node");

    let packages: HashMap<String, LockPackage> =
        serde_json::from_value(packages_value).expect("Failed to convert packages to lock format");

    PackageLock::new(root_node.name.clone(), root_node.version.clone(), packages)
}

/// Serialize graph to package-lock.json packages format.
///
/// Returns (packages_json, total_package_count).
pub fn serialize_to_packages(graph: &DependencyGraph, root_path: &Path) -> (Value, i32) {
    let mut packages = json!({});
    let mut stack = vec![(graph.root_index, String::new())];
    let mut total_packages = 0;

    while let Some((node_index, prefix)) = stack.pop() {
        // Check for duplicate dependencies
        check_duplicate_dependencies(graph, node_index);

        // Create package info
        let pkg_info = create_package_info(graph, node_index, root_path, &mut total_packages);

        // Use empty string for root node
        let key: &str = if prefix.is_empty() { "" } else { &prefix };
        packages[key] = pkg_info;

        // Add physical children to processing stack
        for child_index in graph.get_physical_children(node_index) {
            let child = graph.get_node(child_index).expect("Child node must exist");
            let child_prefix = if prefix.is_empty() {
                if child.is_workspace() {
                    // Workspace nodes use relative path from root
                    child
                        .path
                        .strip_prefix(root_path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| child.path.to_string_lossy().to_string())
                } else {
                    format!("node_modules/{}", child.name)
                }
            } else {
                format!("{}/node_modules/{}", prefix, child.name)
            };
            stack.push((child_index, child_prefix));
        }
    }

    (packages, total_packages)
}

/// Check for duplicate dependencies under a node and log warnings.
fn check_duplicate_dependencies(graph: &DependencyGraph, node_index: NodeIndex) {
    let mut name_count: HashMap<&str, i32> = HashMap::new();
    for child_index in graph.get_physical_children(node_index) {
        if let Some(child) = graph.get_node(child_index)
            && !child.is_link()
        {
            *name_count.entry(&child.name).or_insert(0) += 1;
        }
    }
    for (name, count) in name_count {
        if count > 1
            && let Some(node) = graph.get_node(node_index)
        {
            tracing::debug!(
                "Found {} duplicate dependencies named '{}' under '{}'",
                count,
                name,
                node.name
            );
        }
    }
}

/// Create package information for a node.
fn create_package_info(
    graph: &DependencyGraph,
    node_index: NodeIndex,
    root_path: &Path,
    total_packages: &mut i32,
) -> Value {
    let node = graph.get_node(node_index).expect("Node must exist");

    if node.is_root() {
        create_root_package_info(graph, node_index)
    } else {
        let mut pkg_info =
            create_non_root_package_info(graph, node_index, root_path, total_packages);
        add_package_fields(graph, &mut pkg_info, node_index);
        pkg_info
    }
}

/// Create package info for root node.
fn create_root_package_info(graph: &DependencyGraph, node_index: NodeIndex) -> Value {
    let node = graph.get_node(node_index).expect("Node must exist");
    let manifest = &node.manifest;
    let mut info = json!({
        "name": node.name,
        "version": node.version,
    });

    // Add optional fields from manifest
    if let Some(engines) = manifest.engines() {
        info["engines"] = json!(engines);
    }
    if let Some(workspaces) = manifest.workspaces() {
        info["workspaces"] = workspaces;
    }

    // Add dependency fields
    if let Some(deps) = manifest.dependencies()
        && !deps.is_empty()
    {
        info["dependencies"] = json!(deps);
    }
    if let Some(deps) = manifest.dev_dependencies()
        && !deps.is_empty()
    {
        info["devDependencies"] = json!(deps);
    }
    if let Some(deps) = manifest.peer_dependencies()
        && !deps.is_empty()
    {
        info["peerDependencies"] = json!(deps);
    }
    if let Some(deps) = manifest.optional_dependencies()
        && !deps.is_empty()
    {
        info["optionalDependencies"] = json!(deps);
    }

    info
}

/// Create package info for non-root nodes.
fn create_non_root_package_info(
    graph: &DependencyGraph,
    node_index: NodeIndex,
    root_path: &Path,
    total_packages: &mut i32,
) -> Value {
    let node = graph.get_node(node_index).expect("Node must exist");
    let manifest = &node.manifest;
    let mut info = json!({
        "name": manifest.name(),
    });

    if node.is_workspace() {
        info["version"] = json!(manifest.version());
    } else if node.is_link() {
        info["link"] = json!(true);
        let relative_path = get_relative_path(&node.path, root_path);
        info["resolved"] = json!(relative_path);
    } else {
        // Regular package
        *total_packages += 1;
        info["version"] = json!(manifest.version());

        // Get resolved and integrity from dist field
        if let Some(dist) = manifest.dist() {
            info["resolved"] = json!(dist.tarball);
            info["integrity"] = json!(dist.integrity);
        }
    }

    // Add optional flags
    add_optional_flags(&mut info, node);

    info
}

/// Add optional flags (peer, dev, optional, hasInstallScript).
fn add_optional_flags(info: &mut Value, node: &super::graph::PackageNode) {
    let manifest = &node.manifest;

    if node.is_peer {
        info["peer"] = json!(true);
    }

    match (node.is_dev, node.is_optional) {
        (true, true) => info["devOptional"] = json!(true),
        (true, false) => info["dev"] = json!(true),
        (false, true) => info["optional"] = json!(true),
        _ => {}
    }

    if manifest.has_install_script() {
        info["hasInstallScript"] = json!(true);
    }
}

/// Add package fields like dependencies, bin, license, etc.
fn add_package_fields(graph: &DependencyGraph, pkg_info: &mut Value, node_index: NodeIndex) {
    let node = graph.get_node(node_index).expect("Node must exist");
    let manifest = &node.manifest;

    if let Some(bin) = manifest.bin() {
        pkg_info["bin"] = bin;
    }
    if let Some(license) = manifest.license() {
        pkg_info["license"] = json!(license);
    }
    if let Some(engines) = manifest.engines() {
        pkg_info["engines"] = json!(engines);
    }
    if let Some(os) = manifest.os() {
        pkg_info["os"] = os.clone();
    }
    if let Some(cpu) = manifest.cpu() {
        pkg_info["cpu"] = cpu.clone();
    }
    if let Some(scripts) = manifest.scripts() {
        pkg_info["scripts"] = json!(scripts);
    }
    if manifest.has_install_script() {
        pkg_info["hasInstallScript"] = json!(true);
    }

    // Add dependency fields
    add_dependency_fields(graph, pkg_info, node_index);
}

/// Add dependency fields to package info.
fn add_dependency_fields(graph: &DependencyGraph, pkg_info: &mut Value, node_index: NodeIndex) {
    let dep_types = [
        ("dependencies", EdgeType::Prod),
        ("devDependencies", EdgeType::Dev),
        ("peerDependencies", EdgeType::Peer),
        ("optionalDependencies", EdgeType::Optional),
    ];

    for (field_name, edge_type) in dep_types {
        let deps = collect_dependencies(graph, node_index, &edge_type);
        if !deps.is_empty() {
            pkg_info[field_name] = json!(deps);
        }
    }
}

/// Collect dependencies of a specific type.
fn collect_dependencies(
    graph: &DependencyGraph,
    node_index: NodeIndex,
    edge_type: &EdgeType,
) -> HashMap<String, String> {
    let mut deps = HashMap::new();

    for (_, dep_edge) in graph.get_dependency_edges(node_index) {
        if &dep_edge.edge_type == edge_type {
            deps.insert(dep_edge.name.clone(), dep_edge.spec.clone());
        }
    }

    deps
}

/// Get relative path from root.
fn get_relative_path(path: &Path, root_path: &Path) -> String {
    path.strip_prefix(root_path)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_path_to_pkg_name() {
        assert_eq!(
            LockPackage::path_to_pkg_name("/root/node_modules/a/node_modules/b"),
            Some("b")
        );
        assert_eq!(
            LockPackage::path_to_pkg_name("/root/node_modules/a"),
            Some("a")
        );
        assert_eq!(
            LockPackage::path_to_pkg_name("/root/node_modules/@a/b"),
            Some("@a/b")
        );
        assert_eq!(
            LockPackage::path_to_pkg_name("/root/node_modules/@a/b/node_modules/b/c/d"),
            None
        );
    }

    #[test]
    fn test_lock_package_get_name() {
        let mut package = LockPackage {
            name: Some("test-package".to_string()),
            ..LockPackage::default()
        };

        assert_eq!(
            package.get_name("node_modules/some-package"),
            "test-package"
        );

        package.name = None;
        assert_eq!(package.get_name("node_modules/lodash"), "lodash");
        assert_eq!(
            package.get_name("node_modules/@scope/package"),
            "@scope/package"
        );
        assert_eq!(package.get_name(""), "root");
    }

    #[test]
    fn test_parse_bin_files() {
        let mut package = LockPackage {
            bin: Some(json!({"cli": "bin/cli.js", "tool": "bin/tool.js"})),
            ..LockPackage::default()
        };

        let bin_files = package.parse_bin_files("test-package");
        assert_eq!(bin_files.len(), 2);

        package.bin = Some(json!("index.js"));
        let bin_files = package.parse_bin_files("test-package");
        assert_eq!(bin_files.len(), 1);
        assert_eq!(
            bin_files[0],
            ("test-package".to_string(), "index.js".to_string())
        );

        package.bin = None;
        assert_eq!(package.parse_bin_files("test-package").len(), 0);
    }

    #[test]
    fn test_license_field_parsing() {
        let json_string = r#"{"version": "1.0.0", "license": "MIT"}"#;
        let package: LockPackage = serde_json::from_str(json_string).unwrap();
        assert!(matches!(package.license, Some(License::String(_))));

        let json_array = r#"{"version": "1.0.0", "license": ["MIT", "Apache-2.0"]}"#;
        let package: LockPackage = serde_json::from_str(json_array).unwrap();
        assert!(matches!(package.license, Some(License::Array(_))));
    }

    #[test]
    fn test_package_lock_parsing() {
        let lock_json = r#"{
            "name": "test-project",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-project",
                    "version": "1.0.0"
                },
                "node_modules/lodash": {
                    "version": "4.17.21"
                }
            }
        }"#;

        let package_lock: PackageLock = serde_json::from_str(lock_json).unwrap();
        assert_eq!(package_lock.name, "test-project");
        assert_eq!(package_lock.lockfile_version, 3);
        assert_eq!(package_lock.packages.len(), 2);
    }
}
