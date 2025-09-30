use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use once_cell::sync::Lazy;
use futures;

use crate::helper::lock::{
    PackageLock, build_ideal_tree_to_package_lock, serialize_tree_to_packages,
};
use crate::helper::ruborist::Ruborist;
use crate::helper::workspace::find_workspaces;
use crate::service::registry::RegistryService;
use crate::service::workspace::WorkspaceService;
use crate::util::config::get_legacy_peer_deps;
use crate::util::json::load_package_json_from_path;
use crate::util::logger::{log_verbose, PROGRESS_BAR, start_progress_bar, finish_progress_bar};
use crate::util::registry::store_cache;
use crate::util::semver::matches;

// avoid 422 warning
static CONCURRENCY_LIMITER: Lazy<Arc<Mutex<Arc<Semaphore>>>> = Lazy::new(|| Arc::new(Mutex::new(Arc::new(Semaphore::new(15)))));

/// Set the concurrency limit for dependency resolution
pub fn set_concurrency(limit: usize) {
    let new_semaphore = Arc::new(Semaphore::new(limit));
    if let Ok(mut limiter) = CONCURRENCY_LIMITER.lock() {
        *limiter = new_semaphore;
        log_verbose(&format!("Set concurrency limit to {}", limit));
    }
}

/// Get the current concurrency limiter
fn get_concurrency_limiter() -> Arc<Semaphore> {
    CONCURRENCY_LIMITER.lock().unwrap().clone()
}
#[derive(Debug, Clone)]
struct DependencyNode {
    name: String,
    spec: String,
    parent: Option<String>,
}

impl DependencyNode {
    fn new(name: String, spec: String) -> Self {
        Self {
            name,
            spec,
            parent: None,
        }
    }

    fn new_root(name: String, spec: String) -> Self {
        Self {
            name,
            spec,
            parent: None,
        }
    }

    fn new_workspace(name: String, spec: String) -> Self {
        Self {
            name,
            spec,
            parent: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedDependency {
    name: String,
    resolved_version: String,
}

#[derive(Debug)]
enum ResolutionResult {
    Success {
        resolved: ResolvedDependency,
        children: Vec<DependencyNode>,
    },
    Skipped {
        name: String,
        spec: String,
        reason: String,
    },
    Failed {
        name: String,
        spec: String,
        error: anyhow::Error,
    },
}

/// Dependency resolution service
pub struct DependencyResolutionService;

impl DependencyResolutionService {
    async fn resolve_with_retry(name: &str, spec: &str, max_retries: u32) -> Result<crate::service::registry::ResolvedPackage> {
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match RegistryService::resolve_package(name, spec).await {
                Ok(resolved) => return Ok(resolved),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        let delay = std::time::Duration::from_millis(100 * (attempt + 1) as u64);
                        tokio::time::sleep(delay).await;
                        log_verbose(&format!("Retrying {}/{} for {}@{}", attempt + 1, max_retries, name, spec));
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded")))
    }

    fn should_resolve_dependency(
        name: &str,
        spec: &str,
        resolved_deps: &HashMap<String, ResolvedDependency>,
    ) -> bool {
        if let Some(existing) = resolved_deps.get(name) {
            if matches(spec, &existing.resolved_version) {
                log_verbose(&format!(
                    "Skipping {name}@{spec}: existing version {} satisfies spec",
                    existing.resolved_version
                ));
                return false;
            } else {
                log_verbose(&format!(
                    "Need to resolve {name}@{spec}: existing version {} doesn't satisfy spec",
                    existing.resolved_version
                ));
                return true;
            }
        }

        true
    }

    async fn extract_child_dependencies(
        manifest: &Value,
        parent: &DependencyNode,
    ) -> Result<Vec<DependencyNode>> {
        let mut children = Vec::new();
        let legacy_peer_deps = get_legacy_peer_deps().await;

        let dep_types = if legacy_peer_deps {
            vec!["dependencies", "optionalDependencies"]
        } else {
            vec!["dependencies", "peerDependencies", "optionalDependencies"]
        };

        for dep_type in dep_types {
            if let Some(deps) = manifest.get(dep_type).and_then(|d| d.as_object()) {
                for (name, spec_value) in deps {
                    if let Some(spec) = spec_value.as_str() {
                        let mut child = DependencyNode::new(
                            name.clone(),
                            spec.to_string(),
                        );
                        child.parent = Some(parent.name.clone());
                        children.push(child);
                    }
                }
            }
        }

        log_verbose(&format!(
            "Extracted {} child dependencies from {}",
            children.len(),
            parent.name
        ));

        Ok(children)
    }

    async fn resolve_dependency_with_children(
        dep: DependencyNode,
    ) -> ResolutionResult {
        let limiter = get_concurrency_limiter();
        let _permit = match limiter.acquire().await {
            Ok(permit) => permit,
            Err(e) => {
                return ResolutionResult::Failed {
                    name: dep.name,
                    spec: dep.spec,
                    error: anyhow::anyhow!("Failed to acquire concurrency permit: {}", e),
                };
            }
        };

        let resolve_result = Self::resolve_with_retry(&dep.name, &dep.spec, 3).await;

        match resolve_result {
            Ok(resolved) => {
                log_verbose(&format!(
                    "✓ Resolved: {}@{} => {} (from: {:?})",
                    dep.name,
                    dep.spec,
                    resolved.version,
                    dep.parent.as_deref().unwrap_or("root")
                ));

                let resolved_dep = ResolvedDependency {
                    name: dep.name.clone(),
                    resolved_version: resolved.version,
                };

                match Self::extract_child_dependencies(&resolved.manifest, &dep).await {
                    Ok(children) => {
                        ResolutionResult::Success {
                            resolved: resolved_dep,
                            children,
                        }
                    }
                    Err(e) => {
                        log_verbose(&format!(
                            "Failed to extract children from {}: {}",
                            dep.name, e
                        ));
                        // 即使提取子依赖失败，也返回成功的解析结果
                        ResolutionResult::Success {
                            resolved: resolved_dep,
                            children: vec![],
                        }
                    }
                }
            }
            Err(e) => {

                if dep.spec.starts_with("workspace:") {
                    log_verbose(&format!("○ Skipped workspace dependency: {}@{}", dep.name, dep.spec));
                    ResolutionResult::Skipped {
                        name: dep.name,
                        spec: dep.spec,
                        reason: "workspace dependency".to_string(),
                    }
                } else {
                    log_verbose(&format!("○ Skipped failed dependency (will retry in install): {}@{} - {}", dep.name, dep.spec, e));
                    ResolutionResult::Skipped {
                        name: dep.name,
                        spec: dep.spec,
                        reason: format!("preload failed: {}", e),
                    }
                }
            }
        }
    }

    pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
        let mut ruborist = Ruborist::new(cwd);
        ruborist.build_ideal_tree().await?;

        let pkg_file = load_package_json_from_path(cwd).await?;

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
        // Return PackageLock directly and write to disk asynchronously
        let package_lock = build_ideal_tree_to_package_lock(cwd, &tree, true).await?;

        Ok(package_lock)
    }

    pub async fn build_workspace(cwd: &Path) -> Result<()> {
        // Use the new workspace service to build the JSON
        let workspace_file = WorkspaceService::build_workspace_json(cwd).await?;

        let temp_path = cwd.join("workspace.json.tmp");
        let target_path = cwd.join("workspace.json");

        tokio::fs::write(&temp_path, serde_json::to_string_pretty(&workspace_file)?)
            .await
            .context("Failed to write temporary workspace.json")?;
        tokio::fs::rename(temp_path, target_path)
            .await
            .context("Failed to rename temporary workspace.json")?;

        Ok(())
    }

    pub async fn validate_deps(
        pkg_file: &serde_json::Value,
        pkgs_in_pkg_lock: &serde_json::Value,
    ) -> Result<Vec<crate::helper::lock::InvalidDependency>> {
        crate::helper::lock::validate_deps(pkg_file, pkgs_in_pkg_lock).await
    }

    pub async fn preload(cwd: &Path) -> Result<()> {
        log_verbose("Starting dependency tree preload");

        let mut current_level = Self::collect_root_dependencies(cwd).await?;
        let mut resolved_deps: HashMap<String, ResolvedDependency> = HashMap::new();
        let mut level = 1;

        start_progress_bar();

        while !current_level.is_empty() {
            log_verbose(&format!(
                "Processing dependency level {} with {} nodes",
                level,
                current_level.len()
            ));

            let to_resolve: Vec<DependencyNode> = current_level
                .into_iter()
                .filter(|dep| Self::should_resolve_dependency(&dep.name, &dep.spec, &resolved_deps))
                .collect();

            if to_resolve.is_empty() {
                log_verbose(&format!("Level {} complete - no new dependencies to resolve", level));
                break;
            }

            log_verbose(&format!("Resolving {} dependencies in level {}", to_resolve.len(), level));
            PROGRESS_BAR.inc_length(to_resolve.len() as u64);

            let level_results = Self::resolve_level_dependencies(to_resolve).await;

            let mut next_level = Vec::new();

            for result in level_results {
                match result {
                    ResolutionResult::Success { resolved, children } => {
                        resolved_deps.insert(resolved.name.clone(), resolved);
                        next_level.extend(children);
                    }
                    ResolutionResult::Skipped { name, spec, reason } => {
                        log_verbose(&format!("Skipped {}@{}: {}", name, spec, reason));
                    }
                    ResolutionResult::Failed { name, spec, error } => {
                        log_verbose(&format!("Failed to resolve {}@{}: {}", name, spec, error));
                    }
                }
                PROGRESS_BAR.inc(1);
            }

            current_level = next_level;
            level += 1;
        }

        log_verbose(&format!(
            "Dependency tree traversal complete after {} levels",
            level - 1
        ));

        let node_modules = cwd.join("node_modules");
        if !tokio::fs::try_exists(&node_modules).await? {
            tokio::fs::create_dir_all(&node_modules).await
                .context("Failed to create node_modules directory")?;
        }

        // write cache file asynchronously
        tokio::spawn(async move {
            let cache_path = node_modules.join(".utoo-manifest.json");
            let _ = store_cache(&cache_path).await
                .context("Failed to store preload cache");
        });

        finish_progress_bar(&format!("manifests preloaded"));

        Ok(())
    }

    async fn collect_root_dependencies(cwd: &Path) -> Result<Vec<DependencyNode>> {
        let mut root_deps = Vec::new();
        let mut seen_deps = HashMap::new(); // 去重：(name, spec) -> DependencyNode

        let root_pkg = load_package_json_from_path(cwd).await?;
        let root_dep_types = Self::get_dependency_types_for_node(true, false).await;

        for dep_type in root_dep_types {
            if let Some(deps) = Self::extract_deps_from_package(&root_pkg, dep_type) {
                for (name, spec) in deps {
                    let key = (name.clone(), spec.clone());
                    seen_deps.insert(key, DependencyNode::new_root(
                        name,
                        spec,
                    ));
                }
            }
        }

        match find_workspaces(cwd).await {
            Ok(workspaces) => {
                log_verbose(&format!("Found {} workspace(s)", workspaces.len()));

                for (workspace_name, workspace_path, workspace_pkg) in workspaces {
                    let workspace_dep_types = Self::get_dependency_types_for_node(false, true).await;

                    for dep_type in workspace_dep_types {
                        if let Some(deps) = Self::extract_deps_from_package(&workspace_pkg, dep_type) {
                            for (name, spec) in deps {
                                let key = (name.clone(), spec.clone());
                                seen_deps.entry(key).or_insert_with(|| {
                                    DependencyNode::new_workspace(
                                        name,
                                        spec,
                                    )
                                });
                            }
                        }
                    }

                    log_verbose(&format!("Processed workspace: {} at {}", workspace_name, workspace_path.display()));
                }
            }
            Err(e) => {
                log_verbose(&format!("Failed to find workspaces: {}", e));
            }
        }

        root_deps.extend(seen_deps.into_values());

        log_verbose(&format!("Collected {} unique root dependencies", root_deps.len()));
        Ok(root_deps)
    }

    async fn resolve_level_dependencies(dependencies: Vec<DependencyNode>) -> Vec<ResolutionResult> {
        let futures: Vec<_> = dependencies.into_iter().map(|dep| {
            Self::resolve_dependency_with_children(dep)
        }).collect();

        futures::future::join_all(futures).await
    }

    async fn get_dependency_types_for_node(is_root: bool, is_workspace: bool) -> Vec<&'static str> {
        let legacy_peer_deps = get_legacy_peer_deps().await;

        if is_root || is_workspace {
            if legacy_peer_deps {
                vec!["dependencies", "devDependencies", "optionalDependencies"]
            } else {
                vec!["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"]
            }
        } else {
            if legacy_peer_deps {
                vec!["dependencies", "optionalDependencies"]
            } else {
                vec!["dependencies", "peerDependencies", "optionalDependencies"]
            }
        }
    }

    fn extract_deps_from_package(package_json: &Value, dep_type: &str) -> Option<Vec<(String, String)>> {
        package_json
            .get(dep_type)
            .and_then(|deps| deps.as_object())
            .map(|deps_obj| {
                deps_obj
                    .iter()
                    .filter_map(|(name, spec)| {
                        spec.as_str().map(|spec_str| (name.clone(), spec_str.to_string()))
                    })
                    .collect()
            })
    }

}
