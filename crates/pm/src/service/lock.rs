use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{collections::HashMap, fs};

use crate::helper::workspace::find_workspaces;
use crate::util::config::get_legacy_peer_deps;
use crate::util::json::{load_package_json_from_path, load_package_lock_json_from_path};
use crate::util::logger::{log_verbose, log_warning, log_info};
use crate::model::node::{EdgeType, Node};
use crate::model::override_rule::Overrides;
use crate::helper::registry::{resolve, resolve_dependency};
use crate::util::relative_path::to_relative_path;
use crate::util::save_type::{PackageAction, SaveType};
use crate::util::semver;
use crate::util::{cache::parse_pattern, cloner::clone, downloader::download};

use crate::helper::workspace::find_workspace_path;

#[derive(Deserialize)]
pub struct PackageLock {
    pub packages: HashMap<String, Package>,
}

impl PackageLock {
    pub async fn from_file(path: PathBuf) -> Result<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        let package_lock = serde_json::from_str(&content)?;
        Ok(package_lock)
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Package {
    pub name: Option<String>,
    pub version: Option<String>,
    pub resolved: Option<String>,
    pub link: Option<bool>,
    pub cpu: Option<Value>,
    pub os: Option<Value>,
    pub has_install_script: Option<bool>,
}

#[derive(Debug)]
pub struct InvalidDependency {
    pub package_path: String,
    pub dependency_name: String,
}

/// Lock file service
pub struct LockService;

impl LockService {
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
        Self::normalize_deps_field(pkg_field) == Self::normalize_deps_field(lock_field)
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
                .map_err(|e| anyhow!("Failed to find workspace path: {}", e))?
        } else {
            cwd.to_path_buf()
        };

        // 2. Parse all package specs in parallel
        let mut package_specs = Vec::new();
        for spec in specs {
            let (name, version, version_spec) = Self::parse_package_spec(spec).await?;
            package_specs.push((name, version, version_spec));
        }

        // 3. Read package.json once
        let package_json_path = target_dir.join("package.json");
        let mut package_json: Value = serde_json::from_reader(fs::File::open(&package_json_path)?)?;

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
        fs::write(
            &package_json_path,
            serde_json::to_string_pretty(&package_json)?,
        )?;

        Ok(())
    }

    pub async fn parse_package_spec(spec: &str) -> Result<(String, String, String)> {
        let (name, version_spec) = parse_pattern(spec);
        let resolved = resolve(&name, &version_spec).await?;
        Ok((name, resolved.version, version_spec))
    }

    pub async fn prepare_global_package_json(npm_spec: &str, prefix: Option<&str>) -> Result<PathBuf> {
        // Parse package name and version
        let (name, _version, version_spec) = Self::parse_package_spec(npm_spec).await?;
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
        if !cache_flag_path.exists() {
            log_verbose(&format!(
                "Downloading {} to {}",
                tarball_url,
                cache_path.display()
            ));
            download(tarball_url, &cache_path)
                .await
                .map_err(|e| anyhow!("Failed to download package: {}", e))?;

            // If the package has install scripts, create a flag file
            // in linux, we can use hardlink when FICLONE is not supported
            // so we need to copy the file to the package directory to avoid effect other packages
            if resolved.manifest.get("hasInstallScript") == Some(&json!(true)) {
                let has_install_script_flag_path = cache_path.join("_hasInstallScript");
                fs::write(has_install_script_flag_path, "")?;
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
            .map_err(|e| anyhow!("Failed to clone package: {}", e))?;

        // Remove devDependencies from package.json
        let package_json_path = package_path.join("package.json");
        let mut package_json: Value = serde_json::from_reader(fs::File::open(&package_json_path)?)?;

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
        fs::write(
            &package_json_path,
            serde_json::to_string_pretty(&package_json)?,
        )?;

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

    pub async fn is_pkg_lock_outdated(root_path: &Path) -> Result<bool> {
        let pkg_file = load_package_json_from_path(root_path)?;
        let lock_file = load_package_lock_json_from_path(root_path)?;

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
            for (dep_field, _is_optional) in Self::get_dep_types() {
                if !Self::deps_fields_equal(pkg.get(dep_field), lock.get(dep_field)) {
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
                for (dep_field, is_optional) in Self::get_dep_types() {
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

    fn get_dep_types() -> Vec<(&'static str, bool)> {
        let legacy_peer_deps = get_legacy_peer_deps();

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

    pub async fn write_ideal_tree_to_lock_file(path: &Path, ideal_tree: &Arc<Node>) -> Result<()> {
        let (packages, total_packages) = Self::serialize_tree_to_packages(ideal_tree, path);
        let lock_file = json!({
            "name": ideal_tree.name,  // Direct field access
            "version": ideal_tree.version,  // Direct field access
            "lockfileVersion": 3,
            "requires": true,
            "packages": packages,
        });

        log_info(&format!(
            "Total {total_packages} dependencies after merging"
        ));

        // Write to temporary file first, then atomically move to target location
        let temp_path = path.join("package-lock.json.tmp");
        let target_path = path.join("package-lock.json");

        fs::write(&temp_path, serde_json::to_string_pretty(&lock_file)?)
            .context("Failed to write temporary package-lock.json")?;

        fs::rename(temp_path, target_path).context("Failed to rename temporary package-lock.json")?;

        Ok(())
    }

    pub fn serialize_tree_to_packages(node: &Arc<Node>, path: &Path) -> (Value, i32) {
        let mut packages = json!({});
        let mut stack = vec![(node.clone(), String::new())];
        let mut total_packages = 0;

        while let Some((current, prefix)) = stack.pop() {
            // Check for duplicate dependencies
            Self::check_duplicate_dependencies(&current);

            // Create package info based on node type
            let pkg_info = Self::create_package_info(&current, path, &mut total_packages);

            // Use empty string for root node
            let key = if prefix.is_empty() {
                String::new()
            } else {
                prefix.clone()
            };
            packages[key] = pkg_info;

            // Add children to processing stack
            Self::add_children_to_stack(&current, &prefix, path, &mut stack);
        }

        (packages, total_packages)
    }

    /// Check for duplicate dependencies under a node and log warnings
    fn check_duplicate_dependencies(node: &Arc<Node>) {
        let children = node.children.read().unwrap();
        let mut name_count = HashMap::new();

        for child in children.iter() {
            if !child.is_link {
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
            Self::create_root_package_info(node)
        } else {
            Self::create_non_root_package_info(node, root_path, total_packages)
        };

        // Add package fields (dependencies, bin, license, etc.)
        Self::add_package_fields(&mut pkg_info, node);

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
        } else if node.is_link {
            info["link"] = json!(true);
            let target_path = Self::get_relative_target_path(node, root_path);
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
        Self::add_optional_flags(&mut info, node);

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
        let fields = Self::get_package_fields(node);

        for field in fields {
            if let Some(field_value) = node.package.get(field)
                && Self::should_include_field(field_value)
            {
                pkg_info[field] = field_value.clone();
            }
        }
    }

    /// Get the list of fields to include based on node type
    fn get_package_fields(node: &Arc<Node>) -> Vec<&'static str> {
        if node.is_link {
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
            let child_prefix = Self::generate_child_prefix(prefix, child, root_path);
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
}
