use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::helper::workspace::find_workspaces;
use crate::model::node::{EdgeType, Node};
use crate::model::override_rule::Overrides;
use crate::model::package_lock::LockPackage;
use crate::util::config::get_legacy_peer_deps;
use crate::util::json::{load_package_json_from_path, load_package_lock_json_from_path};
use crate::util::logger::{log_verbose, log_warning};
use crate::util::registry::{resolve, resolve_dependency};
use crate::util::relative_path::to_relative_path;
use crate::util::save_type::{PackageAction, SaveType};
use crate::util::semver;
use crate::util::{cache::parse_pattern, cloner::clone, downloader::download};
use crate::{cmd::deps::build_deps, util::logger::log_info};

use super::workspace::find_workspace_path;

// Use the model's LockPackage but create a simplified PackageLock for helper functions
#[derive(Deserialize, Serialize, Clone)]
pub struct PackageLock {
    pub packages: HashMap<String, LockPackage>,
}

// Type alias for backward compatibility
pub type Package = LockPackage;

pub fn group_by_depth(
    packages: &HashMap<String, Package>,
) -> HashMap<usize, Vec<(String, Package)>> {
    let mut groups = HashMap::new();
    for (path, package) in packages {
        let depth = path.matches("node_modules").count();
        groups
            .entry(depth)
            .or_insert_with(Vec::new)
            .push((path.clone(), package.clone()));
    }
    groups
}

pub fn extract_package_name(path: &str) -> String {
    if let Some(index) = path.rfind("node_modules/") {
        let (_, package_path) = path.split_at(index + "node_modules/".len());
        package_path.to_string()
    } else {
        path.to_string()
    }
}

/// Normalize dependency field: convert empty objects to None for consistent comparison
fn normalize_deps_field(field: Option<&Value>) -> Option<&Value> {
    match field {
        Some(val) if val.as_object().is_some_and(|obj| obj.is_empty()) => None,
        other => other,
    }
}

/// Compare dependency fields, treating empty objects and None as equal
fn deps_fields_equal(pkg_field: Option<&Value>, lock_field: Option<&Value>) -> bool {
    normalize_deps_field(pkg_field) == normalize_deps_field(lock_field)
}

pub async fn ensure_package_lock(root_path: &Path) -> Result<PackageLock> {
    // Check package.json exists in project directory
    if tokio::fs::metadata(root_path.join("package.json"))
        .await
        .is_err()
    {
        return Err(anyhow!("package.json not found"));
    }

    // Check package-lock.json exists in project directory
    if tokio::fs::metadata(root_path.join("package-lock.json"))
        .await
        .is_err()
    {
        log_info("Resolving dependencies");
        // Return PackageLock directly from build_deps, avoiding disk read
        return build_deps(root_path).await;
    } else {
        // Validate dependencies to ensure package-lock.json is in sync with package.json
        if is_pkg_lock_outdated(root_path).await? {
            // Return PackageLock directly from build_deps, avoiding disk read
            return build_deps(root_path).await;
        }

        // Load existing package-lock.json only when it's valid and up-to-date
        log_info("Loading package-lock.json from current project for dependency download");
        let package_lock: PackageLock =
            crate::util::json::read_json_file(&root_path.join("package-lock.json")).await?;

        Ok(package_lock)
    }
}

/// Batch update package.json for multiple package specifications to reduce file I/O operations
pub async fn update_package_json(
    cwd: &Path,
    action: &PackageAction,
    specs: &[&str],
    workspace: &Option<String>,
    save_type: &SaveType,
) -> Result<()> {
    if specs.is_empty() {
        return Ok(());
    }

    // 1. Find target workspace if specified
    let target_dir = if let Some(ws) = workspace {
        find_workspace_path(cwd, ws)
            .await
            .map_err(|e| anyhow!("Failed to find workspace path: {e}"))?
    } else {
        cwd.to_path_buf()
    };

    // 2. Parse all package specs in parallel
    let mut package_specs = Vec::new();
    for spec in specs {
        let (name, version, version_spec) = parse_package_spec(spec).await?;
        package_specs.push((name, version, version_spec));
    }

    // 3. Read package.json once
    let package_json_path = target_dir.join("package.json");
    let package_json_content = tokio::fs::read_to_string(&package_json_path).await?;
    let mut package_json: Value = serde_json::from_str(&package_json_content)?;

    let dep_field = match save_type {
        SaveType::Dev => "devDependencies",
        SaveType::Peer => "peerDependencies",
        SaveType::Optional => "optionalDependencies",
        SaveType::Prod => "dependencies",
    };

    // 4. Ensure dependencies field exists if we're adding packages
    if *action == PackageAction::Add && package_json.get(dep_field).is_none() {
        package_json[dep_field] = Value::Object(serde_json::Map::new());
    }

    // 5. Update all packages in memory
    if let Some(deps) = package_json.get_mut(dep_field)
        && let Some(deps_obj) = deps.as_object_mut()
    {
        for (name, version, version_spec) in package_specs {
            match action {
                PackageAction::Add => {
                    let version_to_write = match version_spec {
                        spec if spec.is_empty() || spec == "*" || spec == "latest" => {
                            format!("^{version}")
                        }
                        spec => spec.to_string(),
                    };
                    deps_obj.insert(name, Value::String(version_to_write));
                }
                PackageAction::Remove => {
                    deps_obj.remove(&name);
                }
            }
        }
    }

    // 6. Write back to package.json once
    tokio::fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json)?,
    )
    .await?;

    Ok(())
}

pub async fn parse_package_spec(spec: &str) -> Result<(String, String, String)> {
    let (name, version_spec) = parse_pattern(spec);
    let resolved = resolve(&name, &version_spec).await?;
    Ok((name, resolved.version, version_spec))
}

pub async fn prepare_global_package_json(npm_spec: &str, prefix: Option<&str>) -> Result<PathBuf> {
    // Parse package name and version
    let (name, _version, version_spec) = parse_package_spec(npm_spec).await?;
    let lib_path = match prefix {
        Some(prefix) => PathBuf::from(prefix).join("lib/node_modules"),
        None => {
            // Get current executable path
            let current_exe = std::env::current_exe()?;
            current_exe
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("lib/node_modules")
        }
    };

    log_verbose(&format!("lib_path: {}", lib_path.to_string_lossy()));

    // Create global package directory
    let package_path = lib_path.join(&name);
    tokio::fs::create_dir_all(&package_path).await?;

    // Get package info from registry
    let resolved = resolve(&name, &version_spec).await?;

    // Get tarball URL from manifest
    let tarball_url = resolved.manifest["dist"]["tarball"]
        .as_str()
        .ok_or_else(|| anyhow!("Failed to get tarball URL from manifest"))?;

    // Download and extract package
    let cache_dir = crate::util::cache::get_cache_dir();
    let cache_path = cache_dir.join(format!("{}/{}", name, resolved.version));
    let cache_flag_path = cache_dir.join(format!("{}/{}/_resolved", name, resolved.version));

    // Download if not cached
    if !tokio::fs::try_exists(&cache_flag_path).await? {
        log_verbose(&format!(
            "Downloading {} to {}",
            tarball_url,
            cache_path.display()
        ));
        download(tarball_url, &cache_path)
            .await
            .map_err(|e| anyhow!("Failed to download package: {e}"))?;

        // If the package has install scripts, create a flag file
        // in linux, we can use hardlink when FICLONE is not supported
        // so we need to copy the file to the package directory to avoid effect other packages
        if resolved.manifest.get("hasInstallScript") == Some(&json!(true)) {
            let has_install_script_flag_path = cache_path.join("_hasInstallScript");
            tokio::fs::write(has_install_script_flag_path, "").await?;
        }
    }

    // Clone to package directory
    log_verbose(&format!(
        "Cloning {} to {}",
        cache_path.display(),
        package_path.display()
    ));
    clone(&cache_path, &package_path, true)
        .await
        .map_err(|e| anyhow!("Failed to clone package: {e}"))?;

    // Remove devDependencies from package.json
    let package_json_path = package_path.join("package.json");
    let package_json_content = tokio::fs::read_to_string(&package_json_path).await?;
    let mut package_json: Value = serde_json::from_str(&package_json_content)?;

    // Remove specified dependency fields and scripts.prepare
    let package_obj = package_json.as_object_mut().unwrap();
    package_obj.remove("devDependencies");

    // Remove scripts.prepare if it exists
    if let Some(scripts) = package_obj.get_mut("scripts")
        && let Some(scripts_obj) = scripts.as_object_mut()
    {
        scripts_obj.remove("prepare");
        scripts_obj.remove("prepublish");
    }

    // Write back the modified package.json
    tokio::fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json)?,
    )
    .await?;

    log_verbose(&format!("package_path: {}", package_path.to_string_lossy()));
    Ok(package_path)
}

/// Extract the relative package name from a package directory path string.
/// Handles both normal and scoped packages, and skips invalid deep paths.
pub fn path_to_pkg_name(path_str: &str) -> Option<&str> {
    if let Some(idx) = path_str.rfind("node_modules/") {
        let pkg_name = &path_str[idx + "node_modules/".len()..];
        let parts: Vec<&str> = pkg_name.split('/').collect();
        // Only allow ora or @scope/ora, skip @pkg/name/path/custom/package.json
        if parts.len() > 2 || (parts.len() == 2 && !parts[0].starts_with('@')) {
            return None;
        }
        Some(pkg_name)
    } else {
        None
    }
}

#[derive(Debug)]
pub struct InvalidDependency {
    pub package_path: String,
    pub dependency_name: String,
}

pub async fn is_pkg_lock_outdated(root_path: &Path) -> Result<bool> {
    let pkg_file = load_package_json_from_path(root_path).await?;
    let lock_file = load_package_lock_json_from_path(root_path).await?;

    // get packages in package-lock.json
    let packages = lock_file
        .get("packages")
        .and_then(|p| p.as_object())
        .ok_or_else(|| anyhow!("Invalid package-lock.json format"))?;

    // prepare packages to check
    let mut pkgs_to_check = vec![("".to_string(), pkg_file.clone())];

    // populate all workspaces
    let workspaces = find_workspaces(root_path).await?;
    for (_, path, workspace_pkg) in workspaces {
        let target_path = to_relative_path(&path, root_path);
        pkgs_to_check.push((target_path, workspace_pkg));
    }

    // new workspace not found
    for (path, pkg) in pkgs_to_check {
        let lock = match packages.get(&path) {
            Some(lock) => lock,
            None => {
                let name = if path.is_empty() { "root" } else { &path };
                log_warning(&format!(
                    "package-lock.json is outdated, new workspace {name} not found"
                ));
                return Ok(true);
            }
        };

        // check dependencies whether changed
        for (dep_field, _is_optional) in get_dep_types().await {
            if !deps_fields_equal(pkg.get(dep_field), lock.get(dep_field)) {
                let name = if path.is_empty() { "root" } else { &path };
                log_warning(&format!(
                    "package-lock.json is outdated, {name} {dep_field} changed"
                ));
                return Ok(true);
            }
        }

        // only check engines for root workspace
        if path.is_empty() && pkg.get("engines") != lock.get("engines") {
            log_warning("package-lock.json is outdated, engines changed");
            return Ok(true);
        }
    }

    Ok(false)
}

pub async fn validate_deps(
    pkg_file: &Value,
    pkgs_in_pkg_lock: &Value,
) -> Result<Vec<InvalidDependency>> {
    let mut invalid_deps = Vec::new();
    // Initialize overrides
    let overrides = Overrides::parse(pkg_file.clone());

    if let Some(packages) = pkgs_in_pkg_lock.as_object() {
        for (pkg_path, pkg_info) in packages {
            for (dep_field, is_optional) in get_dep_types().await {
                if let Some(dependencies) = pkg_info.get(dep_field).and_then(|d| d.as_object()) {
                    for (dep_name, req_version) in dependencies {
                        let req_version_str = req_version.as_str().unwrap_or_default();

                        // Collect parent chain information
                        let mut parent_chain = Vec::new();
                        let mut current_path = String::from(pkg_path);

                        while !current_path.is_empty() {
                            if let Some(pkg_info) = packages.get(&current_path)
                                && let Some(name) = pkg_info.get("name").and_then(|n| n.as_str())
                                && let Some(version) =
                                    pkg_info.get("version").and_then(|v| v.as_str())
                            {
                                parent_chain.push((name.to_string(), version.to_string()));
                            }

                            if let Some(last_modules) = current_path.rfind("/node_modules/") {
                                current_path = current_path[..last_modules].to_string();
                            } else {
                                current_path = String::new();
                            }
                        }

                        // Check if there's an override rule for this dependency
                        let effective_req_version = if let Some(overrides) = &overrides {
                            let mut effective_version = req_version_str.to_string();
                            for rule in &overrides.rules {
                                // Clone the rule to avoid holding the lock across await
                                let rule = rule.clone();
                                let matches = overrides
                                    .matches_rule(&rule, dep_name, req_version_str, &parent_chain)
                                    .await;
                                if matches {
                                    effective_version = rule.target_spec.clone();
                                    break;
                                }
                            }
                            effective_version
                        } else {
                            req_version_str.to_string()
                        };

                        // find the actual version of the dependency
                        let mut current_path = String::from(pkg_path);
                        let mut dep_info = None;

                        // until root or found
                        loop {
                            let search_path = if current_path.is_empty() {
                                format!("node_modules/{dep_name}")
                            } else {
                                format!("{current_path}/node_modules/{dep_name}")
                            };

                            if let Some(info) = packages.get(&search_path) {
                                dep_info = Some(info);
                                current_path = search_path;
                                break;
                            }

                            // find in root path
                            if current_path.is_empty() {
                                break;
                            }

                            // find in parent path
                            if let Some(last_modules) = current_path.rfind("/node_modules/") {
                                current_path = current_path[..last_modules].to_string();
                            } else {
                                current_path = String::new();
                            }
                        }

                        // optional dependency not found is allowed
                        if let Some(dep_info) = dep_info {
                            if let Some(actual_version) =
                                dep_info.get("version").and_then(|v| v.as_str())
                                && !semver::matches(&effective_req_version, actual_version)
                            {
                                if let Some(resolved_dep) = resolve_dependency(
                                    dep_name,
                                    &effective_req_version,
                                    &EdgeType::Optional,
                                )
                                .await?
                                    && resolved_dep.version == actual_version
                                {
                                    log_verbose(&format!(
                                        "Package {pkg_path} {dep_field} dependency {dep_name} (required version: {req_version_str}, effective version: {effective_req_version}) hit bug-version {current_path}@{actual_version}"
                                    ));
                                    continue;
                                }

                                log_verbose(&format!(
                                    "Package {pkg_path} {dep_field} dependency {dep_name} (required version: {req_version_str}, effective version: {effective_req_version}) does not match actual version {current_path}@{actual_version}"
                                ));
                                invalid_deps.push(InvalidDependency {
                                    package_path: pkg_path.to_string(),
                                    dependency_name: dep_name.to_string(),
                                });
                            }
                        } else if !is_optional {
                            log_verbose(&format!(
                                "pkg_path {pkg_path} dep_field {dep_field} dep_name {dep_name} not found"
                            ));
                            invalid_deps.push(InvalidDependency {
                                package_path: pkg_path.to_string(),
                                dependency_name: dep_name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(invalid_deps)
}

async fn get_dep_types() -> Vec<(&'static str, bool)> {
    let legacy_peer_deps = get_legacy_peer_deps().await;

    if legacy_peer_deps {
        vec![
            ("dependencies", false),
            ("optionalDependencies", true),
            ("devDependencies", false),
        ]
    } else {
        vec![
            ("dependencies", false),
            ("peerDependencies", false),
            ("optionalDependencies", true),
            ("devDependencies", false),
        ]
    }
}

/// Convert serialized packages Value to HashMap for PackageLock
fn convert_packages_to_hashmap(packages: Value) -> Result<HashMap<String, Package>> {
    serde_json::from_value(packages).with_context(|| "Failed to parse packages")
}

/// Build ideal tree and return PackageLock, optionally write to disk asynchronously
pub async fn build_ideal_tree_to_package_lock(
    path: &Path,
    ideal_tree: &Arc<Node>,
    write_to_disk: bool,
) -> Result<PackageLock> {
    let (packages, total_packages) = serialize_tree_to_packages(ideal_tree, path);

    log_info(&format!(
        "Total {total_packages} dependencies after merging"
    ));

    // Convert packages Value to HashMap for PackageLock
    let packages_map = convert_packages_to_hashmap(packages.clone())?;
    let package_lock = PackageLock {
        packages: packages_map,
    };

    if write_to_disk {
        // Spawn asynchronous disk writing task
        let path_clone = path.to_path_buf();
        let ideal_tree_name = ideal_tree.name.clone();
        let ideal_tree_version = ideal_tree.version.clone();

        tokio::spawn(async move {
            let lock_file = json!({
                "name": ideal_tree_name,
                "version": ideal_tree_version,
                "lockfileVersion": 3,
                "requires": true,
                "packages": packages,
            });

            let lock_file_content = match serde_json::to_string_pretty(&lock_file) {
                Ok(content) => content,
                Err(e) => {
                    log_verbose(&format!("Failed to serialize package-lock.json: {e}"));
                    return;
                }
            };

            // Write to temporary file first, then atomically move to target location
            let temp_path = path_clone.join("package-lock.json.tmp");
            let target_path = path_clone.join("package-lock.json");

            if let Err(e) = tokio::fs::write(&temp_path, lock_file_content).await {
                log_verbose(&format!("Failed to write package-lock.json: {e}"));
                return;
            }

            if let Err(e) = tokio::fs::rename(temp_path, target_path).await {
                log_verbose(&format!("Failed to rename package-lock.json: {e}"));
            }
        });
    }

    Ok(package_lock)
}

pub fn serialize_tree_to_packages(node: &Arc<Node>, path: &Path) -> (Value, i32) {
    let mut packages = json!({});
    let mut stack = vec![(node.clone(), String::new())];
    let mut total_packages = 0;

    while let Some((current, prefix)) = stack.pop() {
        // Check for duplicate dependencies
        check_duplicate_dependencies(&current);

        // Create package info based on node type
        let pkg_info = create_package_info(&current, path, &mut total_packages);

        // Use empty string for root node
        let key = if prefix.is_empty() {
            String::new()
        } else {
            prefix.clone()
        };
        packages[key] = pkg_info;

        // Add children to processing stack
        add_children_to_stack(&current, &prefix, path, &mut stack);
    }

    (packages, total_packages)
}

/// Check for duplicate dependencies under a node and log warnings
fn check_duplicate_dependencies(node: &Arc<Node>) {
    let children = node.children.read().unwrap();
    let mut name_count = HashMap::new();

    for child in children.iter() {
        if !child.is_link() {
            *name_count.entry(child.name.as_str()).or_insert(0) += 1;
        }
    }

    for (name, count) in name_count {
        if count > 1 {
            log_warning(&format!(
                "Found {} duplicate dependencies named '{}' under '{}'",
                count, name, node.name
            ));
        }
    }
}

/// Create package information based on node type
fn create_package_info(node: &Arc<Node>, root_path: &Path, total_packages: &mut i32) -> Value {
    let mut pkg_info = if node.is_root() {
        create_root_package_info(node)
    } else {
        create_non_root_package_info(node, root_path, total_packages)
    };

    // Add package fields (dependencies, bin, license, etc.)
    add_package_fields(&mut pkg_info, node);

    pkg_info
}

/// Create package info for root node
fn create_root_package_info(node: &Arc<Node>) -> Value {
    let mut info = json!({
        "name": node.name,
        "version": node.version,
    });

    if let Some(engines) = node.package.get("engines") {
        info["engines"] = engines.clone();
    }

    info
}

/// Create package info for non-root nodes
fn create_non_root_package_info(
    node: &Arc<Node>,
    root_path: &Path,
    total_packages: &mut i32,
) -> Value {
    let mut info = json!({
        "name": node.package.get("name"),
    });

    if node.is_workspace() {
        info["version"] = json!(node.package.get("version"));
    } else if node.is_link() {
        info["link"] = json!(true);
        let target_path = get_relative_target_path(node, root_path);
        info["resolved"] = json!(target_path);
    } else {
        // Regular package
        info["version"] = json!(node.package.get("version"));

        let empty_dist = json!("");
        let dist = node.package.get("dist").unwrap_or(&empty_dist);
        info["resolved"] = json!(dist.get("tarball"));
        info["integrity"] = json!(dist.get("integrity"));

        *total_packages += 1;
    }

    // Add optional flags
    add_optional_flags(&mut info, node);

    info
}

/// Add optional flags (peer, dev, optional, hasInstallScript)
fn add_optional_flags(info: &mut Value, node: &Arc<Node>) {
    if *node.is_peer.read().unwrap() == Some(true) {
        info["peer"] = json!(true);
    }

    let is_dev = *node.is_dev.read().unwrap() == Some(true);
    let is_optional = *node.is_optional.read().unwrap() == Some(true);

    match (is_dev, is_optional) {
        (true, true) => info["devOptional"] = json!(true),
        (true, false) => info["dev"] = json!(true),
        (false, true) => info["optional"] = json!(true),
        _ => {}
    }

    if node.package.get("hasInstallScript") == Some(&json!(true)) {
        info["hasInstallScript"] = json!(true);
    }
}

/// Add package fields based on node type
fn add_package_fields(pkg_info: &mut Value, node: &Arc<Node>) {
    let fields = get_package_fields(node);

    for field in fields {
        if let Some(field_value) = node.package.get(field)
            && should_include_field(field_value)
        {
            pkg_info[field] = field_value.clone();
        }
    }
}

/// Get the list of fields to include based on node type
fn get_package_fields(node: &Arc<Node>) -> Vec<&'static str> {
    if node.is_link() {
        vec![]
    } else if node.is_root() {
        vec![
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ]
    } else {
        let mut fields = vec![
            "dependencies",
            "peerDependencies",
            "optionalDependencies",
            "bin",
            "license",
            "engines",
            "os",
            "cpu",
        ];

        if node.is_workspace() {
            fields.push("devDependencies");
        }

        fields
    }
}

/// Check if a field value should be included in the output
fn should_include_field(field_value: &Value) -> bool {
    if field_value.is_object() {
        !field_value.as_object().unwrap().is_empty()
    } else {
        true // Include non-object values (strings, etc.)
    }
}

/// Add children to the processing stack
fn add_children_to_stack(
    node: &Arc<Node>,
    prefix: &str,
    root_path: &Path,
    stack: &mut Vec<(Arc<Node>, String)>,
) {
    let children = node.children.read().unwrap();

    for child in children.iter() {
        let child_prefix = generate_child_prefix(prefix, child, root_path);
        stack.push((child.clone(), child_prefix));
    }
}

/// Generate the prefix path for a child node
fn generate_child_prefix(prefix: &str, child: &Arc<Node>, root_path: &Path) -> String {
    if prefix.is_empty() {
        if child.is_workspace() {
            // Convert workspace path to relative path
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
    }
}

/// Get the relative path of a link target from the root path
fn get_relative_target_path(current: &Node, root_path: &Path) -> String {
    let target = current.target.read().unwrap();
    let target_node = target.as_ref().unwrap();

    // Try to get relative path first
    target_node
        .path
        .strip_prefix(root_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| target_node.path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::Node;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_version_to_write() {
        // Test cases for different version specifications
        let test_cases = vec![
            ("1.2.3", "", "^1.2.3"),
            ("1.2.3", "*", "^1.2.3"),
            ("1.2.3", "latest", "^1.2.3"),
            ("1.2.3", "^1.2.0", "^1.2.0"),
            ("1.2.3", "~1.2.0", "~1.2.0"),
            ("1.2.3", "1.2.3", "1.2.3"),
        ];

        for (version, spec, expected) in test_cases {
            let version_to_write = match spec {
                spec if spec.is_empty() || spec == "*" || spec == "latest" => {
                    format!("^{version}")
                }
                spec => spec.to_string(),
            };
            assert_eq!(
                version_to_write, expected,
                "Failed for version: {version}, spec: {spec}",
            );
        }
    }

    #[test]
    fn test_path_to_pkg_name() {
        // Normal nested package
        assert_eq!(
            super::path_to_pkg_name("/root/node_modules/a/node_modules/b"),
            Some("b")
        );
        // Top-level package
        assert_eq!(super::path_to_pkg_name("/root/node_modules/a"), Some("a"));

        assert_eq!(
            super::path_to_pkg_name("/root/node_modules/@a/b"),
            Some("@a/b")
        );
        // Deep invalid path (should be None)
        assert_eq!(
            super::path_to_pkg_name("/root/node_modules/@a/b/node_modules/b/c/d"),
            None
        );
    }

    #[tokio::test]
    async fn test_validate_deps_with_invalid_dependencies() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            }
        });

        // Create a mock package-lock.json structure
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "lodash": "^4.17.20"
                }
            },
            "node_modules/lodash": {
                "name": "lodash",
                "version": "3.17.20",
                "resolved": "https://registry.npmjs.org/lodash/-/lodash-3.17.20.tgz"
            }
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        assert!(!invalid_deps.is_empty());
    }

    #[tokio::test]
    async fn test_validate_deps_with_valid_dependencies() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            }
        });

        // Create a mock package-lock.json structure
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "lodash": "^4.17.20"
                }
            },
            "node_modules/lodash": {
                "name": "lodash",
                "version": "4.17.20",
                "resolved": "https://registry.npmjs.org/lodash/-/lodash-4.17.20.tgz"
            }
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        assert!(invalid_deps.is_empty());
    }

    #[tokio::test]
    async fn test_build_ideal_tree_to_package_lock() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create a mock ideal tree
        let root = Node::new(
            "test-package".to_string(),
            temp_path.to_path_buf(),
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "lodash": "^4.17.20"
                }
            }),
        );

        // Test building PackageLock from ideal tree with disk write
        let result = build_ideal_tree_to_package_lock(temp_path, &root, true).await;
        assert!(result.is_ok());

        let package_lock = result.unwrap();

        // Verify PackageLock structure
        assert!(package_lock.packages.contains_key(""));

        // Give the async task time to write to disk
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify the lock file was created on disk
        let lock_file_path = temp_path.join("package-lock.json");
        assert!(lock_file_path.exists());

        // Read and verify the content
        let content = fs::read_to_string(lock_file_path).unwrap();
        let lock_data: Value = serde_json::from_str(&content).unwrap();

        assert_eq!(lock_data["name"], "test-package");

        // Verify packages section
        let packages = lock_data["packages"].as_object().unwrap();
        assert!(packages.contains_key(""));
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Test case 1: package.json and package-lock.json are in sync
        let pkg_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            }
        });

        let pkg_lock = json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0",
                    "dependencies": {
                        "lodash": "^4.17.20"
                    },
                    "devDependencies": {
                        "typescript": "^4.9.0"
                    }
                }
            }
        });

        // Write test files to temporary directory
        fs::write(temp_path.join("package.json"), pkg_json.to_string()).unwrap();
        fs::write(temp_path.join("package-lock.json"), pkg_lock.to_string()).unwrap();

        // Test that files are in sync
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test case 2: package.json has new dependency
        let pkg_json_updated = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20",
                "react": "^18.0.0"  // New dependency
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            }
        });

        fs::write(temp_path.join("package.json"), pkg_json_updated.to_string()).unwrap();
        let outdated = is_pkg_lock_outdated(temp_path).await.unwrap();
        assert!(outdated);

        // Test case 3: package.json has updated version
        let pkg_json_version_updated = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.21"  // Updated version
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            }
        });

        fs::write(
            temp_path.join("package.json"),
            pkg_json_version_updated.to_string(),
        )
        .unwrap();
        assert!(is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test case 4: package.json has removed dependency
        let pkg_json_removed = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            }
            // Removed devDependencies
        });

        fs::write(temp_path.join("package.json"), pkg_json_removed.to_string()).unwrap();
        assert!(is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test case 4: package.json has removed dependency
        let pkg_json_engines_changed = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            },
            "engines": {
                "install-node": "16"
            }
            // Removed devDependencies
        });

        fs::write(
            temp_path.join("package.json"),
            pkg_json_engines_changed.to_string(),
        )
        .unwrap();
        assert!(is_pkg_lock_outdated(temp_path).await.unwrap());
    }

    #[test]
    fn test_get_relative_target_path() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        // Test case 1: Target path is under root path
        let target_path = root_path.join("packages/pkg-a");
        let node = Node::new(
            "test-package".to_string(),
            root_path.to_path_buf(),
            json!({
                "name": "test-package",
                "version": "1.0.0"
            }),
        );
        node.target.write().unwrap().replace(Node::new(
            "target-package".to_string(),
            target_path.clone(),
            json!({
                "name": "target-package",
                "version": "1.0.0"
            }),
        ));

        let relative_path = get_relative_target_path(&node, root_path);
        assert_eq!(relative_path, "packages/pkg-a");

        // Test case 2: Target path is outside root path
        let outside_path = PathBuf::from("/some/outside/path");
        let node = Node::new(
            "test-package".to_string(),
            root_path.to_path_buf(),
            json!({
                "name": "test-package",
                "version": "1.0.0"
            }),
        );
        node.target.write().unwrap().replace(Node::new(
            "target-package".to_string(),
            outside_path.clone(),
            json!({
                "name": "target-package",
                "version": "1.0.0"
            }),
        ));

        let relative_path = get_relative_target_path(&node, root_path);
        assert_eq!(relative_path, "/some/outside/path");

        // Test case 3: Target path is the root path
        let node = Node::new(
            "test-package".to_string(),
            root_path.to_path_buf(),
            json!({
                "name": "test-package",
                "version": "1.0.0"
            }),
        );
        node.target.write().unwrap().replace(Node::new(
            "target-package".to_string(),
            root_path.to_path_buf(),
            json!({
                "name": "target-package",
                "version": "1.0.0"
            }),
        ));

        let relative_path = get_relative_target_path(&node, root_path);
        assert_eq!(relative_path, "");
    }

    #[test]
    fn test_serialize_tree_to_packages_with_workspace_bin() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create root package.json
        fs::write(
            temp_path.join("package.json"),
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "workspaces": ["packages/*"]
            })
            .to_string(),
        )
        .unwrap();

        // Create workspace package directory and package.json
        let workspace_path = temp_path.join("packages/workspace-a");
        fs::create_dir_all(&workspace_path).unwrap();
        fs::write(
            workspace_path.join("package.json"),
            json!({
                "name": "workspace-a",
                "version": "1.0.0",
                "bin": {
                    "workspace-a": "./bin/index.js",
                    "workspace-a-cli": "./bin/cli.js"
                }
            })
            .to_string(),
        )
        .unwrap();

        // Create bin directory and files
        let bin_path = workspace_path.join("bin");
        fs::create_dir_all(&bin_path).unwrap();
        fs::write(bin_path.join("index.js"), "console.log('workspace-a');").unwrap();
        fs::write(bin_path.join("cli.js"), "console.log('workspace-a-cli');").unwrap();

        // Create root node
        let root = Node::new(
            "test-package".to_string(),
            temp_path.to_path_buf(),
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "workspaces": ["packages/*"]
            }),
        );

        // Create workspace node
        let workspace = Node::new_workspace(
            "workspace-a".to_string(),
            workspace_path.clone(),
            json!({
                "name": "workspace-a",
                "version": "1.0.0",
                "bin": {
                    "workspace-a": "./bin/index.js",
                    "workspace-a-cli": "./bin/cli.js"
                }
            }),
        );
        root.children.write().unwrap().push(workspace);

        // Test serialization
        let (packages, _) = serialize_tree_to_packages(&root, temp_path);

        // Verify workspace package
        let workspace_pkg = packages.get("packages/workspace-a").unwrap();
        println!("workspace_pkg: {workspace_pkg:?}");
        assert_eq!(workspace_pkg["name"], "workspace-a");

        // Verify bin configuration
        let bin = workspace_pkg["bin"].as_object().unwrap();
        assert_eq!(bin["workspace-a"], "./bin/index.js");
        assert_eq!(bin["workspace-a-cli"], "./bin/cli.js");
    }

    #[tokio::test]
    async fn test_validate_deps_with_version_mismatch() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "example-pkg": "^2.0.0"
            }
        });

        // Create a mock package-lock.json structure with version mismatch
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "example-pkg": "^2.0.0"
                }
            },
            "node_modules/example-pkg": {
                "name": "example-pkg",
                "version": "1.5.0",  // Doesn't match ^2.0.0
                "resolved": "https://registry.npmjs.org/example-pkg/-/example-pkg-1.5.0.tgz"
            }
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        assert_eq!(invalid_deps.len(), 1);
        assert_eq!(invalid_deps[0].package_path, "");
        assert_eq!(invalid_deps[0].dependency_name, "example-pkg");
    }

    #[tokio::test]
    async fn test_validate_deps_with_optional_dependency_missing() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "optionalDependencies": {
                "optional-pkg": "^1.0.0"
            }
        });

        // Create a mock package-lock.json structure with missing optional dependency
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "optionalDependencies": {
                    "optional-pkg": "^1.0.0"
                }
            }
            // optional-pkg is missing from node_modules
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        // Optional dependency missing should not be treated as invalid
        assert_eq!(invalid_deps.len(), 0);
    }

    #[tokio::test]
    async fn test_validate_deps_with_required_dependency_missing() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "required-pkg": "^1.0.0"
            }
        });

        // Create a mock package-lock.json structure with missing required dependency
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "required-pkg": "^1.0.0"
                }
            }
            // required-pkg is missing from node_modules
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        // Required dependency missing should be treated as invalid
        assert_eq!(invalid_deps.len(), 1);
        assert_eq!(invalid_deps[0].package_path, "");
        assert_eq!(invalid_deps[0].dependency_name, "required-pkg");
    }

    #[tokio::test]
    async fn test_validate_deps_with_peer_dependencies() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "peerDependencies": {
                "peer-pkg": "^3.0.0"
            }
        });

        // Create a mock package-lock.json structure
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "peerDependencies": {
                    "peer-pkg": "^3.0.0"
                }
            },
            "node_modules/peer-pkg": {
                "name": "peer-pkg",
                "version": "3.1.0",  // Matches ^3.0.0
                "resolved": "https://registry.npmjs.org/peer-pkg/-/peer-pkg-3.1.0.tgz"
            }
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        // Valid peer dependency should not be treated as invalid
        assert_eq!(invalid_deps.len(), 0);
    }

    #[tokio::test]
    async fn test_validate_deps_with_nested_dependencies() {
        // Create a mock package.json
        let pkg_file = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "parent-pkg": "^1.0.0"
            }
        });

        // Create a mock package-lock.json structure with nested dependencies
        let pkgs_in_pkg_lock = json!({
            "": {
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "parent-pkg": "^1.0.0"
                }
            },
            "node_modules/parent-pkg": {
                "name": "parent-pkg",
                "version": "1.2.0",
                "resolved": "https://registry.npmjs.org/parent-pkg/-/parent-pkg-1.2.0.tgz",
                "dependencies": {
                    "nested-pkg": "^2.0.0"
                }
            },
            "node_modules/parent-pkg/node_modules/nested-pkg": {
                "name": "nested-pkg",
                "version": "2.1.0",  // Matches ^2.0.0
                "resolved": "https://registry.npmjs.org/nested-pkg/-/nested-pkg-2.1.0.tgz"
            }
        });

        let invalid_deps = validate_deps(&pkg_file, &pkgs_in_pkg_lock).await.unwrap();
        // All dependencies are valid
        assert_eq!(invalid_deps.len(), 0);
    }

    #[test]
    fn test_package_struct_with_name_field() {
        // Test that Package struct can be deserialized with name field
        let package_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "resolved": "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz",
            "link": false
        });

        let package: Package = serde_json::from_value(package_json).unwrap();
        assert_eq!(package.name, Some("test-package".to_string()));
        assert_eq!(package.version, Some("1.0.0".to_string()));
        assert_eq!(
            package.resolved,
            Some("https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string())
        );
        assert_eq!(package.link, Some(false));
    }

    #[test]
    fn test_package_struct_without_name_field() {
        // Test that Package struct can be deserialized without name field (backward compatibility)
        let package_json = json!({
            "version": "1.0.0",
            "resolved": "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz",
            "link": false
        });

        let package: Package = serde_json::from_value(package_json).unwrap();
        assert_eq!(package.name, None);
        assert_eq!(package.version, Some("1.0.0".to_string()));
        assert_eq!(
            package.resolved,
            Some("https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string())
        );
        assert_eq!(package.link, Some(false));
    }

    #[test]
    fn test_deps_fields_equal() {
        // Test case 1: Both None
        assert!(deps_fields_equal(None, None));

        // Test case 2: Both empty objects
        let empty_obj = json!({});
        assert!(deps_fields_equal(Some(&empty_obj), Some(&empty_obj)));

        // Test case 3: None vs empty object
        assert!(deps_fields_equal(None, Some(&empty_obj)));
        assert!(deps_fields_equal(Some(&empty_obj), None));

        // Test case 4: Both have same non-empty content
        let deps1 = json!({
            "lodash": "^4.17.20",
            "react": "^18.0.0"
        });
        let deps2 = json!({
            "lodash": "^4.17.20",
            "react": "^18.0.0"
        });
        assert!(deps_fields_equal(Some(&deps1), Some(&deps2)));

        // Test case 5: Different content
        let deps3 = json!({
            "lodash": "^4.17.20"
        });
        let deps4 = json!({
            "react": "^18.0.0"
        });
        assert!(!deps_fields_equal(Some(&deps3), Some(&deps4)));

        // Test case 6: Non-empty vs None
        let deps5 = json!({
            "lodash": "^4.17.20"
        });
        assert!(!deps_fields_equal(Some(&deps5), None));
        assert!(!deps_fields_equal(None, Some(&deps5)));

        // Test case 7: Non-empty vs empty object
        assert!(!deps_fields_equal(Some(&deps5), Some(&empty_obj)));
        assert!(!deps_fields_equal(Some(&empty_obj), Some(&deps5)));

        // Test case 8: Non-object values
        let string_val = json!("some-string");
        let number_val = json!(123);
        assert!(deps_fields_equal(Some(&string_val), Some(&string_val)));
        assert!(!deps_fields_equal(Some(&string_val), Some(&number_val)));
        assert!(!deps_fields_equal(Some(&string_val), None));
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_with_empty_deps() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Test case: package.json has empty dependencies object, package-lock.json has no dependencies field
        let pkg_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {}  // Empty object
        });

        let pkg_lock = json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0"
                    // No dependencies field
                }
            }
        });

        // Write test files to temporary directory
        fs::write(temp_path.join("package.json"), pkg_json.to_string()).unwrap();
        fs::write(temp_path.join("package-lock.json"), pkg_lock.to_string()).unwrap();

        // Test that empty object and missing field are treated as equal
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test reverse case: package.json has no dependencies field, package-lock.json has empty dependencies
        let pkg_json_no_deps = json!({
            "name": "test-package",
            "version": "1.0.0"
            // No dependencies field
        });

        let pkg_lock_empty_deps = json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0",
                    "dependencies": {}  // Empty object
                }
            }
        });

        fs::write(temp_path.join("package.json"), pkg_json_no_deps.to_string()).unwrap();
        fs::write(
            temp_path.join("package-lock.json"),
            pkg_lock_empty_deps.to_string(),
        )
        .unwrap();

        // Test that missing field and empty object are treated as equal
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());
    }
}
