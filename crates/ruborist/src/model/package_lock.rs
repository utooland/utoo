//! Package-lock.json data structures.
//!
//! Shared types for serializing/deserializing npm lock files.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::util::{PackageNameStr, deserialize_or_default};

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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_or_default"
    )]
    pub engines: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<HashMap<String, String>>,
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
            if parts.len() > 2 || (parts.len() == 2 && !parts[0].is_scoped()) {
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

    /// Check if package has install scripts.
    pub fn has_install_scripts(&self) -> bool {
        self.has_install_script.unwrap_or(false)
    }

    /// Whether this entry is a symlink node — a workspace `node_modules` link
    /// or a `file:<dir>` dependency (both serialize as `"link": true`).
    pub fn is_link(&self) -> bool {
        self.link == Some(true)
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
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        packages: HashMap<String, LockPackage>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
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

use super::graph::DependencyGraph;
use super::node::EdgeType;

/// Serialize a dependency graph to PackageLock format.
///
/// This is the main entry point for converting a resolved dependency graph
/// into a package-lock.json compatible structure.
pub fn serialize_graph(graph: &DependencyGraph, root_path: &Path) -> PackageLock {
    let (packages, _total) = serialize_to_packages(graph, root_path);

    let root_node = graph
        .get_node(graph.root_index)
        .expect("Graph must have a root node");

    PackageLock::new(&root_node.name, &root_node.version, packages)
}

/// Serialize graph to package-lock.json packages format.
///
/// Builds `LockPackage` structs directly (no intermediate `serde_json::Value`).
/// Returns (packages_map, total_package_count).
pub fn serialize_to_packages(
    graph: &DependencyGraph,
    root_path: &Path,
) -> (HashMap<String, LockPackage>, i32) {
    let mut packages = HashMap::new();
    let mut stack = vec![(graph.root_index, String::new())];
    let mut total_packages = 0;

    while let Some((node_index, prefix)) = stack.pop() {
        // Check for duplicate dependencies
        check_duplicate_dependencies(graph, node_index);

        // Create package info
        let lock_pkg = create_lock_package(graph, node_index, root_path, &mut total_packages);

        packages.insert(prefix.clone(), lock_pkg);

        // Add physical children to processing stack
        for child_index in graph.get_physical_children(node_index) {
            let child = graph.get_node(child_index).expect("Child node must exist");
            let child_prefix = if prefix.is_empty() {
                if child.is_workspace() {
                    child
                        .path
                        .strip_prefix(root_path)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| child.path.to_string_lossy().into_owned())
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

/// Create a LockPackage from a graph node.
fn create_lock_package(
    graph: &DependencyGraph,
    node_index: NodeIndex,
    root_path: &Path,
    total_packages: &mut i32,
) -> LockPackage {
    let node = graph.get_node(node_index).expect("Node must exist");

    if node.is_root() {
        create_root_lock_package(graph, node_index)
    } else {
        create_non_root_lock_package(graph, node_index, root_path, total_packages)
    }
}

/// Create LockPackage for root node.
fn create_root_lock_package(graph: &DependencyGraph, node_index: NodeIndex) -> LockPackage {
    let node = graph.get_node(node_index).expect("Node must exist");
    let manifest = &node.manifest;

    let mut pkg = LockPackage {
        name: Some(node.name.clone()),
        version: Some(node.version.clone()),
        engines: manifest.engines().cloned(),
        workspaces: manifest
            .workspaces()
            .and_then(|v| serde_json::from_value(v).ok()),
        ..LockPackage::default()
    };

    collect_edge_deps(graph, node_index, &mut pkg);

    pkg
}

/// Create LockPackage for non-root nodes.
fn create_non_root_lock_package(
    graph: &DependencyGraph,
    node_index: NodeIndex,
    root_path: &Path,
    total_packages: &mut i32,
) -> LockPackage {
    let node = graph.get_node(node_index).expect("Node must exist");
    let manifest = &node.manifest;

    let mut pkg = LockPackage {
        name: Some(manifest.name().to_string()),
        ..LockPackage::default()
    };

    if node.is_workspace() {
        pkg.version = Some(manifest.version().to_string());
    } else if node.is_link() {
        // Workspace links and `file:<dir>` deps both use `NodeType::Link`;
        // `node.path` is the on-disk source in both cases.
        pkg.link = Some(true);
        pkg.resolved = Some(get_relative_path(&node.path, root_path));
    } else {
        // Regular package
        *total_packages += 1;
        pkg.version = Some(manifest.version().to_string());

        if let Some(dist) = manifest.dist() {
            pkg.resolved = dist
                .tarball
                .as_deref()
                .map(|t| rewrite_resolved(t, root_path));
            pkg.integrity = dist.integrity.clone();
        }
    }

    // Flags
    if node.is_peer {
        pkg.peer = Some(true);
    }
    if node.is_dev {
        pkg.dev = Some(true);
    }
    if node.is_optional {
        pkg.optional = Some(true);
    }

    // Package metadata. `bin` is recorded even on link nodes because utoo's
    // bin-linking reads it straight from the lock entry (the graph/target isn't
    // available at rebuild time).
    pkg.bin = manifest.bin();
    pkg.license = manifest.license().map(License::String);
    pkg.engines = manifest.engines().cloned();
    pkg.os = manifest.os().cloned();
    pkg.cpu = manifest.cpu().cloned();

    // Script markers are NOT stamped on link nodes — npm keeps link entries
    // minimal, and the link's source/target owns the lifecycle: a workspace runs
    // via the workspace walk, and a `file:<dir>` dep is read from disk at collect
    // time. Stamping them here is what produced the #3097 duplicate execution.
    if !node.is_link() {
        if manifest.has_install_script() {
            pkg.has_install_script = Some(true);
        }
        pkg.scripts = manifest.scripts().cloned();
    }

    // Dependencies from graph edges
    collect_edge_deps(graph, node_index, &mut pkg);

    pkg
}

/// Populate dependency fields on LockPackage from graph edges.
/// For root nodes, edges resolved to workspace nodes are excluded.
fn collect_edge_deps(graph: &DependencyGraph, node_index: NodeIndex, pkg: &mut LockPackage) {
    let is_root = graph.get_node(node_index).is_some_and(|n| n.is_root());
    for (_, dep_edge) in graph.get_dependency_edges(node_index) {
        if is_root && graph.is_workspace_target(dep_edge) {
            continue;
        }
        let map = match dep_edge.edge_type {
            EdgeType::Prod => &mut pkg.dependencies,
            EdgeType::Dev => &mut pkg.dev_dependencies,
            EdgeType::Peer => &mut pkg.peer_dependencies,
            EdgeType::Optional => &mut pkg.optional_dependencies,
        };
        map.get_or_insert_with(HashMap::new)
            .insert(dep_edge.name.clone(), dep_edge.spec.clone());
    }
}

/// Compute `path` relative to `root_path`, using POSIX-style separators so
/// the lockfile is identical across platforms. Falls back to `path` as-is
/// when neither is absolute (npm convention for already-relative `resolved`
/// values).
fn get_relative_path(path: &Path, root_path: &Path) -> String {
    let rel = pathdiff::diff_paths(path, root_path).unwrap_or_else(|| path.to_path_buf());
    rel.to_string_lossy().replace('\\', "/")
}

/// Rewrite a `dist.tarball` value for the lockfile's `resolved` field.
///
/// `file:<abs>` URLs stamped by the file tarball resolver are stored
/// absolute in memory but must serialize as `file:<root-relative>` to match
/// npm's format and keep the lockfile portable. Registry HTTPS tarballs
/// and git URLs pass through unchanged.
fn rewrite_resolved(tarball: &str, root_path: &Path) -> String {
    let Some(abs) = tarball.strip_prefix("file:") else {
        return tarball.to_string();
    };
    format!("file:{}", get_relative_path(Path::new(abs), root_path))
}

#[cfg(test)]
mod tests {
    use super::super::graph::PackageNode;
    use super::super::package_json::PackageJson;
    use super::*;
    use std::path::PathBuf;

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

    /// Helper: create a graph with a single non-root node, return its LockPackage.
    fn lock_pkg_for_node(is_dev: bool, is_optional: bool, is_peer: bool) -> LockPackage {
        use super::super::graph::DependencyGraph;

        let root_pkg = PackageJson::new("root", "1.0.0");
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("/root"), root_pkg);

        let child_pkg = PackageJson::new("test", "1.0.0");
        let mut child =
            PackageNode::from_package_json("test".to_string(), PathBuf::from("/root"), child_pkg);
        child.is_dev = is_dev;
        child.is_optional = is_optional;
        child.is_peer = is_peer;
        let child_idx = graph.add_node(child);
        graph.add_physical_edge(graph.root_index, child_idx);

        let (packages, _) = serialize_to_packages(&graph, &PathBuf::from("/root"));
        packages
            .get("node_modules/test")
            .cloned()
            .expect("child package must exist")
    }

    #[test]
    fn test_flags_dev_only() {
        let pkg = lock_pkg_for_node(true, false, false);
        assert_eq!(pkg.dev, Some(true));
        assert!(pkg.optional.is_none());
        assert!(pkg.dev_optional.is_none());
    }

    #[test]
    fn test_flags_optional_only() {
        let pkg = lock_pkg_for_node(false, true, false);
        assert!(pkg.dev.is_none());
        assert_eq!(pkg.optional, Some(true));
        assert!(pkg.dev_optional.is_none());
    }

    #[test]
    fn test_flags_dev_and_optional() {
        let pkg = lock_pkg_for_node(true, true, false);
        assert_eq!(pkg.dev, Some(true));
        assert_eq!(pkg.optional, Some(true));
        assert!(pkg.dev_optional.is_none());
    }

    #[test]
    fn test_flags_peer() {
        let pkg = lock_pkg_for_node(false, false, true);
        assert_eq!(pkg.peer, Some(true));
        assert!(pkg.dev.is_none());
        assert!(pkg.optional.is_none());
    }

    /// Helper: build a workspace graph and return the serialized packages map.
    ///
    /// Graph layout (mirrors npm behaviour):
    ///   root (has dep: lodash ^4.0.0, workspaces: ["packages/*"])
    ///     ├── workspace-a (workspace child — NOT in root dependencies)
    ///     └── lodash (regular child)
    ///
    /// The resolver also creates graph edges for both lodash and workspace-a,
    /// but the root lockfile entry should only contain lodash.
    fn build_workspace_graph() -> HashMap<String, LockPackage> {
        use super::super::graph::DependencyGraph;

        let mut root_pkg = PackageJson::new("my-project", "1.0.0");
        // Only regular deps in manifest — workspace packages are NOT listed
        // in dependencies; they're discovered from the workspaces field.
        root_pkg.dependencies = Some(HashMap::from([(
            "lodash".to_string(),
            "^4.0.0".to_string(),
        )]));
        root_pkg.workspaces =
            Some(serde_json::from_value(serde_json::json!(["packages/*"])).unwrap());

        let root_path = PathBuf::from("/project");
        let mut graph = DependencyGraph::from_package_json(root_path.clone(), root_pkg);

        // Add workspace child
        let ws_pkg = PackageJson::new("workspace-a", "1.0.0");
        let ws_node = PackageNode::workspace_from_package_json(
            PathBuf::from("/project/packages/workspace-a"),
            ws_pkg,
        );
        let ws_idx = graph.add_node(ws_node);
        graph.add_physical_edge(graph.root_index, ws_idx);

        // Add regular child
        let lodash_pkg = PackageJson::new("lodash", "4.17.21");
        let lodash_node = PackageNode::from_package_json(
            "lodash".to_string(),
            PathBuf::from("/project/node_modules/lodash"),
            lodash_pkg,
        );
        let lodash_idx = graph.add_node(lodash_node);
        graph.add_physical_edge(graph.root_index, lodash_idx);

        // The resolver creates edges for ALL deps including workspace packages,
        // and marks them resolved. The lockfile root entry should exclude workspace ones.
        graph.add_dependency_edge(graph.root_index, "lodash", "^4.0.0", EdgeType::Prod);
        let ws_edge =
            graph.add_dependency_edge(graph.root_index, "workspace-a", "^1.0.0", EdgeType::Prod);
        graph.mark_dependency_resolved(ws_edge, ws_idx);

        let (packages, _) = serialize_to_packages(&graph, &root_path);
        packages
    }

    #[test]
    fn test_root_excludes_workspace_deps() {
        let packages = build_workspace_graph();
        let root = packages.get("").expect("root entry must exist");

        let deps = root.dependencies.as_ref().expect("root should have deps");
        assert_eq!(deps.get("lodash").unwrap(), "^4.0.0");
        assert!(
            !deps.contains_key("workspace-a"),
            "workspace package must not appear in root dependencies"
        );
    }

    #[test]
    fn test_root_has_workspaces_field() {
        let packages = build_workspace_graph();
        let root = packages.get("").expect("root entry must exist");

        let workspaces = root
            .workspaces
            .as_ref()
            .expect("root should have workspaces");
        assert_eq!(workspaces, &vec!["packages/*".to_string()]);
    }

    #[test]
    fn test_workspace_child_is_serialized() {
        let packages = build_workspace_graph();

        let ws = packages
            .get("packages/workspace-a")
            .expect("workspace entry must exist");
        assert_eq!(ws.name.as_deref(), Some("workspace-a"));
        assert_eq!(ws.version.as_deref(), Some("1.0.0"));
        assert!(ws.link.is_none(), "workspace node is not a link");
    }

    /// Regression for #3097: a link entry must stay npm-minimal — it keeps `bin`
    /// (utoo's bin-linking reads it from the lock) but must NOT carry the script
    /// markers (`has_install_script`/`scripts`). Stamping those on the link is
    /// what made `collect` run a workspace's install scripts a second time.
    #[test]
    fn test_link_node_omits_script_markers() {
        use super::super::graph::DependencyGraph;

        let root_pkg = PackageJson::new("my-project", "1.0.0");
        let root_path = PathBuf::from("/project");
        let mut graph = DependencyGraph::from_package_json(root_path.clone(), root_pkg);

        // A link node whose package declares a bin, scripts, and a literal
        // hasInstallScript — none of the script markers should survive.
        let mut link_pkg = PackageJson::new("linked", "1.0.0");
        link_pkg.bin = Some(serde_json::json!({ "linked-cli": "cli.js" }));
        link_pkg.scripts = Some(HashMap::from([(
            "postinstall".to_string(),
            "echo hi".to_string(),
        )]));
        link_pkg.has_install_script = Some(true);
        let link_idx = graph.add_node(PackageNode::link_from_package_json(
            PathBuf::from("/project/packages/linked"),
            link_pkg,
        ));
        graph.add_physical_edge(graph.root_index, link_idx);

        let (packages, _) = serialize_to_packages(&graph, &root_path);
        let link = packages
            .get("node_modules/linked")
            .expect("link entry must exist");

        assert_eq!(link.link, Some(true));
        assert!(link.resolved.is_some(), "link records its resolved target");
        assert!(link.bin.is_some(), "link keeps its bin for bin-linking");
        assert!(
            link.has_install_script.is_none(),
            "link must not carry has_install_script"
        );
        assert!(link.scripts.is_none(), "link must not carry scripts");
    }

    #[test]
    fn test_root_no_extra_metadata() {
        let packages = build_workspace_graph();
        let root = packages.get("").expect("root entry must exist");

        // npm root entry does not include bin, license, scripts, os, cpu
        assert!(root.bin.is_none());
        assert!(root.license.is_none());
        assert!(root.scripts.is_none());
        assert!(root.os.is_none());
        assert!(root.cpu.is_none());
        assert!(root.has_install_script.is_none());
    }

    #[test]
    fn test_root_resolves_catalog_specs() {
        use super::super::graph::DependencyGraph;

        let mut root_pkg = PackageJson::new("catalog-project", "1.0.0");
        root_pkg.dependencies = Some(HashMap::from([
            ("react".to_string(), "catalog:default".to_string()),
            ("lodash".to_string(), "^4.0.0".to_string()),
        ]));

        let root_path = PathBuf::from("/project");
        let mut graph = DependencyGraph::from_package_json(root_path.clone(), root_pkg);

        // Edge for react has resolved spec (as the resolver would do)
        graph.add_dependency_edge(graph.root_index, "react", "^18.2.0", EdgeType::Prod);
        graph.add_dependency_edge(graph.root_index, "lodash", "^4.0.0", EdgeType::Prod);

        let (packages, _) = serialize_to_packages(&graph, &root_path);
        let root = packages.get("").expect("root entry must exist");
        let deps = root.dependencies.as_ref().unwrap();

        assert_eq!(
            deps.get("react").unwrap(),
            "^18.2.0",
            "catalog: spec should be resolved"
        );
        assert_eq!(
            deps.get("lodash").unwrap(),
            "^4.0.0",
            "non-catalog spec stays unchanged"
        );
    }
}
