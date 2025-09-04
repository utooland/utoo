use anyhow::{Context, Result};
use std::path::Path;
use std::collections::HashMap;

use crate::helper::lock::{
    serialize_tree_to_packages, write_ideal_tree_to_lock_file,
    Package, extract_package_name, path_to_pkg_name
};
use crate::helper::ruborist::Ruborist;
use crate::helper::workspace::find_workspaces;
use crate::helper::{is_cpu_compatible, is_os_compatible};
use crate::service::workspace::WorkspaceService;
use crate::util::cloner::clone;
use crate::util::downloader::download;
use crate::util::linker::link;
use crate::util::logger::{PROGRESS_BAR, log_progress, log_verbose};
use crate::util::relative_path::to_relative_path;
use crate::util::semver;
use crate::util::json::load_package_json_from_path;

use crate::model::override_rule::Overrides;
use crate::model::node::EdgeType;
use crate::helper::registry::resolve_dependency;
use crate::util::json::load_package_lock_json_from_path;
use crate::util::logger::log_warning;
use crate::util::config::get_legacy_peer_deps;
use serde_json::Value;

use super::binary::update_package_binary;
use tokio::sync::Semaphore;
use std::sync::Arc;

/// Dependency resolution service
pub struct DependencyResolutionService;

impl DependencyResolutionService {
    /// Clean up a single node_modules directory
    async fn clean_node_modules_dir(
        node_modules: &Path,
        cwd: &Path,
        valid_packages: &std::collections::HashSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // clean up symlinks for npminstall
        if let Ok(mut entries) = tokio::fs::read_dir(node_modules).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_symlink() {
                    Self::clean_symlink(&path).await?;
                } else if path.is_dir() {
                    Self::clean_directory(&path).await?;
                }
            }
        }

        Self::clean_unused_packages(node_modules, cwd, valid_packages).await?;

        Ok(())
    }

    /// Clean up a symlink
    async fn clean_symlink(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        log_verbose(&format!("Removing symlink: {}", path.display()));
        if let Err(e) = tokio::fs::remove_file(path).await {
            log_verbose(&format!(
                "Failed to remove symlink {}: {}",
                path.display(),
                e
            ));
        }
        Ok(())
    }

    /// Clean up a directory, handling scoped packages and legacy npm install packages
    async fn clean_directory(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(file_name) = path.file_name()
            && let Some(name) = file_name.to_str()
        {
            if name.starts_with('@') {
                Self::clean_scoped_package(path).await?;
            } else {
                Self::clean_legacy_npminstall_package(path, name).await?;
            }
        }
        Ok(())
    }

    /// Clean up a scoped package directory
    async fn clean_scoped_package(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(mut scope_entries) = tokio::fs::read_dir(path).await {
            while let Ok(Some(scope_entry)) = scope_entries.next_entry().await {
                let scope_path = scope_entry.path();
                if scope_path.is_symlink() {
                    log_verbose(&format!(
                        "Removing scoped symlink: {}",
                        scope_path.display()
                    ));
                    if let Err(e) = tokio::fs::remove_file(&scope_path).await {
                        log_verbose(&format!(
                            "Failed to remove scoped symlink {}: {}",
                            scope_path.display(),
                            e
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Clean up a legacy npminstall package
    async fn clean_legacy_npminstall_package(
        path: &Path,
        name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let at_count = name.matches('@').count();
        if name.starts_with('_') && (at_count == 2 || at_count == 4) {
            log_verbose(&format!("Removing legacy package: {}", path.display()));
            if let Err(e) = tokio::fs::remove_dir_all(path).await {
                log_verbose(&format!(
                    "Failed to remove legacy package {}: {}",
                    path.display(),
                    e
                ));
            }
        }
        Ok(())
    }

    /// Clean up unused packages in the node_modules directory
    async fn clean_unused_packages(
        node_modules: &Path,
        cwd: &Path,
        valid_packages: &std::collections::HashSet<String>,
    ) -> Result<()> {
        // Helper function for recursive search
        fn find_and_clean<'a>(
            node_modules: &'a Path,
            cwd: &'a Path,
            valid_packages: &'a std::collections::HashSet<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
            Box::pin(async move {
                let patterns = [
                    node_modules.join("*/package.json"),
                    node_modules.join("@*/*/package.json"),
                ];
                for pattern in patterns.iter() {
                    let pattern_str = pattern.to_string_lossy().to_string();
                    for entry in glob::glob(&pattern_str)
                        .with_context(|| format!("Glob failed for pattern: {pattern_str}"))?
                    {
                        let pkg_json_path = entry
                            .with_context(|| format!("Glob entry error for pattern: {pattern_str}"))?;
                        let pkg_dir = pkg_json_path
                            .parent()
                            .context("Failed to get parent directory of package.json")?;
                        if let Some(pkg_name) = path_to_pkg_name(&pkg_dir.to_string_lossy()) {
                            let pkg_path = pkg_dir.strip_prefix(cwd).with_context(|| {
                                format!(
                                    "Failed to strip prefix {} from {}",
                                    cwd.display(),
                                    pkg_dir.display()
                                )
                            })?;
                            if !valid_packages.contains(pkg_path.to_string_lossy().as_ref()) {
                                log_verbose(&format!("Cleaning unused package: {pkg_name}"));
                                if let Err(e) = tokio::fs::remove_dir_all(pkg_dir).await {
                                    log_verbose(&format!("Failed to remove {pkg_name}: {e}"));
                                }
                            }
                        }
                        // Recursively check nested node_modules
                        let nested_node_modules = pkg_dir.join("node_modules");
                        if nested_node_modules.exists() {
                            find_and_clean(&nested_node_modules, cwd, valid_packages).await?;
                        }
                    }
                }
                Ok(())
            })
        }
        find_and_clean(node_modules, cwd, valid_packages).await?;
        Ok(())
    }

    async fn clean_deps(
        groups: &HashMap<usize, Vec<(String, Package)>>,
        cwd: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut valid_packages = std::collections::HashSet::new();
        for packages in groups.values() {
            for (path, _) in packages {
                valid_packages.insert(path.clone());
            }
        }

        log_verbose(&format!("Valid packages: {valid_packages:?}"));

        let mut node_modules_dirs = vec![cwd.join("node_modules")];

        let workspaces = find_workspaces(cwd).await?;
        for (_, path, _) in workspaces {
            let workspace_node_modules = path.join("node_modules");
            if workspace_node_modules.exists() {
                node_modules_dirs.push(workspace_node_modules.clone());
                log_verbose(&format!(
                    "add workspace node_modules: {:?}",
                    workspace_node_modules.display()
                ));
            }
        }

        // cleanup unused packages in all workspace_members
        for node_modules in node_modules_dirs {
            Self::clean_node_modules_dir(&node_modules, cwd, &valid_packages).await?;
        }

        Ok(())
    }

    pub async fn install_packages(
        groups: &HashMap<usize, Vec<(std::string::String, Package)>>,
        cache_dir: &Path,
        cwd: &Path,
        semaphore: Arc<Semaphore>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // clean unused deps
        Self::clean_deps(groups, cwd).await?;

        let mut depths: Vec<_> = groups.keys().cloned().collect();
        depths.sort_unstable();

        for depth in depths.iter() {
            if let Some(packages) = groups.get(depth) {
                let mut tasks: Vec<
                    tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
                > = Vec::new();
                for (path, package) in packages.iter() {
                    let path = path.clone();
                    let package = package.clone();
                    if let Some(resolved) = package.resolved {
                        if package.link.is_some() {
                            let link_name = extract_package_name(&path);
                            if link_name.is_empty() {
                                PROGRESS_BAR.inc(1);
                                log_verbose(&format!("Link skipped due to empty package name: {path}"));
                                continue;
                            }
                            log_verbose(&format!("Attempting to link from {resolved} to {path}"));
                            if let Err(e) = link(Path::new(&resolved), Path::new(&path)) {
                                log_verbose(&format!(
                                    "Link failed: source={resolved}, target={path}, error={e}"
                                ));
                                return Err(format!("Link failed: {e}").into());
                            }
                            PROGRESS_BAR.inc(1);
                            continue;
                        }

                        // skip when cpu or os is not compatible
                        if package.cpu.is_some() && !is_cpu_compatible(&package.cpu.unwrap()) {
                            PROGRESS_BAR.inc(1);
                            log_verbose(&format!("cpu skipped: {}", &path));
                            continue;
                        }

                        if package.os.is_some() && !is_os_compatible(&package.os.unwrap()) {
                            PROGRESS_BAR.inc(1);
                            log_verbose(&format!("os skipped: {}", &path));
                            continue;
                        }

                        let name = package.name.unwrap_or_else(|| extract_package_name(&path));
                        let version = package.version.as_ref().unwrap();
                        let cache_path = cache_dir.join(format!("{name}/{version}"));
                        let cache_flag_path = cache_dir.join(format!("{name}/{version}/_resolved"));
                        let cwd_clone = cwd.to_path_buf();
                        let should_resolve = !cache_flag_path.exists();
                        let semaphore = Arc::clone(&semaphore);

                        let task = tokio::spawn(async move {
                            let _permit = semaphore.acquire().await.unwrap();
                            if should_resolve {
                                log_verbose(&format!("Downloading {path} to {name}"));
                                match download(&resolved, &cache_path).await {
                                    Ok(_) => {
                                        log_progress(&format!("{name} downloaded"));
                                        if package.has_install_script.is_some() {
                                            log_verbose(&format!("{name} has install script"));
                                            let has_install_script_flag_path =
                                                cache_path.join("_hasInstallScript");
                                            tokio::fs::write(has_install_script_flag_path, "").await?;
                                        }
                                    }
                                    Err(e) => {
                                        log_verbose(&format!(
                                            "Download failed: source={}, target={}, error={}",
                                            resolved,
                                            cache_path.display(),
                                            e
                                        ));
                                        return Err(Box::new(std::io::Error::other(format!(
                                            "{name} download failed: {e}"
                                        )))
                                            as Box<dyn std::error::Error + Send + Sync>);
                                    }
                                }
                            }

                            log_verbose(&format!("{name} clone"));
                            match clone(&cache_path, &cwd_clone.join(&path), true).await {
                                Ok(_) => {
                                    log_verbose(&format!("{name} resolved"));
                                    PROGRESS_BAR.inc(1);
                                    log_progress(&format!("{name} resolved"));
                                    update_package_binary(&cwd_clone.join(&path), &name).await?;
                                    Ok(())
                                }
                                Err(e) => Err(format!(
                                    "Copy failed {} to {}: {}",
                                    cache_path.display(),
                                    cwd_clone.join(&path).display(),
                                    e
                                )
                                .into()),
                            }
                        });
                        tasks.push(task);
                    } else {
                        PROGRESS_BAR.inc(1);
                        log_verbose(&format!("{path} no resolved info skipped"));
                    }
                }

                for task in tasks {
                    match task.await {
                        Ok(Ok(())) => continue,
                        Ok(Err(e)) => {
                            log_verbose(&format!("Task execution error: {e}"));
                            return Err(format!("Error during installation: {e}").into());
                        }
                        Err(e) => {
                            log_verbose(&format!("Task join error: {e}"));
                            return Err(format!("Task execution failed: {e}").into());
                        }
                    }
                }
            }
        }

        Ok(())
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

    pub async fn is_pkg_lock_outdated(root_path: &Path) -> Result<bool> {
        let pkg_file = load_package_json_from_path(root_path)?;
        let lock_file = load_package_lock_json_from_path(root_path)?;

        // get packages in package-lock.json
        let packages = lock_file
            .get("packages")
            .and_then(|p| p.as_object())
            .ok_or_else(|| anyhow::anyhow!("Invalid package-lock.json format"))?;

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
    ) -> Result<Vec<crate::helper::lock::InvalidDependency>> {
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
                                    invalid_deps.push(crate::helper::lock::InvalidDependency {
                                        package_path: pkg_path.to_string(),
                                        dependency_name: dep_name.to_string(),
                                    });
                                }
                            } else if !is_optional {
                                log_verbose(&format!(
                                    "pkg_path {pkg_path} dep_field {dep_field} dep_name {dep_name} not found"
                                ));
                                invalid_deps.push(crate::helper::lock::InvalidDependency {
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

    pub async fn build_deps(cwd: &Path) -> Result<()> {
        let mut ruborist = Ruborist::new(cwd);
        ruborist.build_ideal_tree().await?;

        let pkg_file = load_package_json_from_path(cwd)?;

        const MAX_RETRIES: u32 = 5;
        let mut retry_count = 0;

        loop {
            let (pkgs_in_tree, _) = {
                let to_guard = ruborist.ideal_tree.as_ref().unwrap();
                serialize_tree_to_packages(to_guard, cwd)
            };

            let invalid_deps = Self::validate_deps(&pkg_file, &pkgs_in_tree).await?;

            if invalid_deps.is_empty() {
                log_verbose("No invalid dependencies found");
                break;
            }

            if retry_count >= MAX_RETRIES {
                return Err(anyhow::anyhow!(
                    "Failed to fix dependencies after {} retries",
                    MAX_RETRIES
                ));
            }

            for dep in invalid_deps {
                log_verbose(&format!(
                    "Fixing dependency: {}/{}",
                    dep.package_path, dep.dependency_name
                ));
                // Try to fix the dependency
                if let Err(e) = ruborist
                    .fix_dep_path(&dep.package_path, &dep.dependency_name)
                    .await
                {
                    log_verbose(&format!("Failed to fix dependency: {e}"));
                    return Err(anyhow::anyhow!("Failed to fix dependency: {}", e));
                } else {
                    log_verbose(&format!(
                        "Fixed dependency: {}/{}",
                        dep.package_path, dep.dependency_name
                    ));
                }
            }

            retry_count += 1;
        }

        let tree = ruborist.ideal_tree.unwrap();
        write_ideal_tree_to_lock_file(cwd, &tree).await?;

        Ok(())
    }

    pub async fn build_workspace(cwd: &Path) -> Result<()> {
        // Use the new workspace service to build the JSON
        let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;

        let temp_path = cwd.join("workspace.json.tmp");
        let target_path = cwd.join("workspace.json");

        std::fs::write(&temp_path, serde_json::to_string_pretty(&workspace_file)?)
            .context("Failed to write temporary workspace.json")?;
        std::fs::rename(temp_path, target_path).context("Failed to rename temporary workspace.json")?;

        Ok(())
    }
}
