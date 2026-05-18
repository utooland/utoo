use crate::util::cli_enum::ScriptPolicy;
use anyhow::{Context as _, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use crate::cmd::deps::build_deps;
use crate::fs;
use crate::helper::global_bin::get_global_bin_dir;
use crate::helper::lock::{
    Package, UpdatePackageJsonOptions, extract_package_name, group_by_depth, is_pkg_lock_outdated,
    prepare_global_package_json, save_package_lock, update_package_json,
};
use crate::helper::ruborist_context::{Context, spawn_save_project_cache};
use crate::helper::workspace::init_project_root;
use crate::model::package::PackageInfo;
use crate::service::rebuild::RebuildService;
use crate::util::cli_enum::{OmitType, PackageAction, SaveType};
use crate::util::cloner::clone_count;
use crate::util::downloader::{download_stats, is_registry_tarball_url};
use crate::util::json::load_package_lock_json_from_path;
use crate::util::linker::link;
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_progress, print_install_counts, start_progress_bar,
};
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};
use utoo_ruborist::spec::SpecStr;

use super::binary::update_package_binary;
use super::clean::clean_deps;

struct FreshLockRegistryPackage {
    path: String,
    name: String,
    version: String,
    resolved: String,
}

/// Check if a package should be omitted based on omit config
fn should_omit_package(package: &Package, omit: &HashSet<OmitType>) -> bool {
    if omit.is_empty() {
        return false;
    }

    let is_dev = package.dev == Some(true);
    let is_optional = package.optional == Some(true);
    let is_dev_optional = package.dev_optional == Some(true);
    let is_peer = package.peer == Some(true);

    // devOptional: only omit when both dev and optional are omitted
    if is_dev_optional {
        return omit.contains(&OmitType::Dev) && omit.contains(&OmitType::Optional);
    }

    // dev only
    if is_dev && omit.contains(&OmitType::Dev) {
        return true;
    }

    // optional only
    if is_optional && omit.contains(&OmitType::Optional) {
        return true;
    }

    // peer
    if is_peer && omit.contains(&OmitType::Peer) {
        return true;
    }

    false
}

fn is_package_platform_compatible(package: &Package) -> bool {
    if let Some(ref cpu) = package.cpu
        && !is_cpu_compatible(cpu)
    {
        return false;
    }

    if let Some(ref os) = package.os
        && !is_os_compatible(os)
    {
        return false;
    }

    true
}

fn dependency_spec<'a>(package: &'a Package, name: &str) -> Option<&'a str> {
    package
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(name))
        .or_else(|| {
            package
                .optional_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
        .or_else(|| {
            package
                .dev_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
        .or_else(|| {
            package
                .peer_dependencies
                .as_ref()
                .and_then(|deps| deps.get(name))
        })
        .map(String::as_str)
}

fn lock_parent_path(path: &str) -> Option<&str> {
    let index = path.rfind("node_modules/")?;
    if index == 0 {
        Some("")
    } else {
        path[..index].strip_suffix('/')
    }
}

fn has_registry_incoming_spec(packages: &HashMap<String, Package>, path: &str, name: &str) -> bool {
    let Some(parent_path) = lock_parent_path(path) else {
        return false;
    };
    let Some(parent) = packages.get(parent_path) else {
        return false;
    };

    dependency_spec(parent, name).is_some_and(|spec| spec.is_registry_spec())
}

fn collect_fresh_lock_registry_packages(
    packages: &HashMap<String, Package>,
) -> Vec<FreshLockRegistryPackage> {
    let mut prefetches = Vec::new();

    for (path, package) in packages {
        if path.is_empty() || package.link.is_some() || !is_package_platform_compatible(package) {
            continue;
        }

        let (Some(version), Some(resolved)) = (&package.version, &package.resolved) else {
            continue;
        };

        if !is_registry_tarball_url(resolved) {
            continue;
        }

        let name = package.get_name(path);
        if name == "unknown" || !has_registry_incoming_spec(packages, path, &name) {
            continue;
        }

        prefetches.push(FreshLockRegistryPackage {
            path: path.clone(),
            name,
            version: version.clone(),
            resolved: resolved.clone(),
        });
    }

    prefetches
}

fn prefetch_fresh_lock_downloads(
    package_lock: &utoo_ruborist::lock::PackageLock,
    scheduler: &super::install_scheduler::InstallScheduler,
) -> HashSet<String> {
    let packages = collect_fresh_lock_registry_packages(&package_lock.packages);
    let mut registry_paths = HashSet::with_capacity(packages.len());

    for package in packages {
        registry_paths.insert(package.path);
        scheduler.prefetch_download(package.name, package.version, package.resolved);
    }

    registry_paths
}

async fn install_packages(
    groups: &HashMap<usize, Vec<(String, Package)>>,
    cwd: &Path,
    omit: &HashSet<OmitType>,
    scheduler: &super::install_scheduler::InstallScheduler,
    registry_clone_paths: &HashSet<String>,
) -> Result<()> {
    // Surface the clean step in the spinner — it doesn't move `pos`, so
    // without a message the bar looks frozen on large trees.
    log_progress("validating node_modules");
    clean_deps(groups, cwd).await?;
    log_progress("linking packages");

    // Always process level-by-level to ensure parent directories exist before
    // children. Within each level, tasks run concurrently. The install
    // scheduler owns clone/download dedupe, so package tasks only request the
    // concrete target they need.
    let mut depths: Vec<_> = groups.keys().cloned().collect();
    depths.sort_unstable();

    for depth in depths.iter() {
        let mut clone_tasks = FuturesUnordered::new();

        if let Some(packages) = groups.get(depth) {
            for (path, package) in packages.iter() {
                // Skip packages based on omit config
                if should_omit_package(package, omit) {
                    PROGRESS_BAR.inc(1);
                    continue;
                }
                let path = path.clone();
                let package = package.clone();
                if let Some(ref resolved) = package.resolved {
                    // Lockfile stores `file:` URLs root-relative (npm format).
                    // Cloner only understands absolute URLs — re-absolutize
                    // here so the cloner/downloader stays unaware of project
                    // root plumbing.
                    let resolved = match resolved.strip_prefix("file:") {
                        Some(rel) if !Path::new(rel).is_absolute() => {
                            format!("file:{}", cwd.join(rel).display())
                        }
                        _ => resolved.clone(),
                    };
                    if package.link.is_some() {
                        let link_name = extract_package_name(&path);
                        if link_name.is_empty() {
                            PROGRESS_BAR.inc(1);
                            continue;
                        }
                        link(Path::new(&resolved), Path::new(&path))
                            .await
                            .with_context(|| format!("Link failed: {resolved} -> {path}"))?;
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    if !is_package_platform_compatible(&package) {
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    let name = package.get_name(&path);
                    let version = package
                        .version
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("package {name} missing version"))?;
                    let cwd_clone = cwd.to_path_buf();
                    let target_path = cwd_clone.join(&path);
                    let scheduler = scheduler.clone();
                    let is_registry_clone = registry_clone_paths.contains(&path);

                    // Check if this is an optional dependency
                    let is_optional =
                        package.optional == Some(true) || package.dev_optional == Some(true);

                    clone_tasks.push(async move {
                        let clone_result = if is_registry_clone {
                            scheduler
                                .ensure_registry_clone(
                                    name.clone(),
                                    version,
                                    resolved,
                                    target_path.clone(),
                                )
                                .await
                        } else {
                            scheduler
                                .ensure_clone(name.clone(), version, resolved, target_path.clone())
                                .await
                        };

                        if let Err(e) = clone_result {
                            if is_optional {
                                tracing::warn!(
                                    "Optional dependency {name} failed (ignored): {e:#}"
                                );
                                PROGRESS_BAR.inc(1);
                                return Ok(());
                            }
                            return Err(e);
                        }
                        PROGRESS_BAR.inc(1);
                        log_progress(&format!("{name} resolved"));
                        update_package_binary(&target_path, &name).await
                    });
                } else {
                    PROGRESS_BAR.inc(1);
                }
            }
        }

        while let Some(result) = clone_tasks.next().await {
            result?;
        }
    }

    Ok(())
}

async fn resolve_package_lock_with_scheduler(
    root_path: &Path,
    scheduler: super::install_scheduler::InstallScheduler,
) -> Result<utoo_ruborist::lock::PackageLock> {
    let options = Context::install_deps_options(root_path.to_path_buf(), scheduler).await;
    let output = utoo_ruborist::service::build_deps(options).await?;

    save_package_lock(root_path, &output.lock).await?;
    spawn_save_project_cache(root_path.to_path_buf(), output.project_cache);

    Ok(output.lock)
}

pub struct InstallService;

impl InstallService {
    pub async fn update_packages(
        action: PackageAction,
        specs: &[&str],
        workspace: Option<String>,
        scripts: ScriptPolicy,
        save_type: SaveType,
        omit: &HashSet<OmitType>,
    ) -> Result<()> {
        tracing::debug!(
            "update packages: {:?} {:?} {:?} {:?}",
            action,
            specs,
            &workspace,
            scripts
        );

        if specs.is_empty() {
            return Err(anyhow::anyhow!("No package specifications provided"));
        }

        let cwd = std::env::current_dir().context("Failed to get current directory")?;

        // Update working directory to project root (if in workspace)
        let root_path = init_project_root(&cwd).await?;

        // Update package.json and package-lock.json for all packages in batch
        update_package_json(&UpdatePackageJsonOptions {
            cwd: &root_path,
            action,
            specs,
            workspace: workspace.as_deref(),
            save_type,
        })
        .await
        .context("Failed to update package.json")?;

        // Rebuild dependencies - the result will be used by install() via ensure_package_lock()
        build_deps(&root_path)
            .await
            .context("Failed to build package-lock.json")?;

        Self::install(scripts, &root_path, omit)
            .await
            .context("Failed to install packages")?;

        Ok(())
    }

    pub async fn install(
        scripts: ScriptPolicy,
        root_path: &Path,
        omit: &HashSet<OmitType>,
    ) -> Result<()> {
        // Snapshot counts so nested install() calls (e.g. global install)
        // report only their own delta instead of the whole process total.
        let clone_baseline = clone_count();
        let download_baseline = download_stats();

        let lock_path = root_path.join("package-lock.json");
        // Treat a failing freshness check as stale: regenerate rather than
        // install from a lockfile we couldn't validate. `is_pkg_lock_outdated`
        // itself emits a `tracing::warn` with the specific mismatch reason.
        let use_fresh_lock = fs::try_exists(&lock_path).await.unwrap_or(false)
            && !is_pkg_lock_outdated(root_path).await.unwrap_or(true);
        let scheduler_handle = super::install_scheduler::InstallSchedulerHandle::start();
        let scheduler = scheduler_handle.scheduler();

        let (package_lock, used_scheduler_prefetch, registry_clone_paths) = if use_fresh_lock {
            let lock = match load_package_lock_json_from_path(root_path).await {
                Ok(lock) => lock,
                Err(e) => {
                    scheduler_handle.shutdown().await;
                    return Err(e);
                }
            };
            let registry_clone_paths = prefetch_fresh_lock_downloads(&lock, &scheduler);
            (lock, true, registry_clone_paths)
        } else {
            start_progress_bar();
            let resolve_start = Instant::now();
            let lock = match resolve_package_lock_with_scheduler(root_path, scheduler.clone()).await
            {
                Ok(lock) => lock,
                Err(e) => {
                    scheduler_handle.shutdown().await;
                    return Err(e);
                }
            };
            finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));
            (lock, true, HashSet::new())
        };

        let groups = group_by_depth(&package_lock.packages);

        if !package_lock.packages.is_empty() {
            start_progress_bar();
            PROGRESS_BAR.set_length(package_lock.packages.len() as u64);
        }

        let link_start = Instant::now();
        let install_result =
            install_packages(&groups, root_path, omit, &scheduler, &registry_clone_paths)
                .await
                .context("Failed to install packages");

        scheduler_handle.shutdown().await;
        if used_scheduler_prefetch {
            super::install_scheduler::print_summary();
        }
        install_result?;
        finish_progress_bar("node_modules cloned", Some(link_start.elapsed()));

        RebuildService::rebuild(&package_lock, root_path, scripts).await?;

        let added = clone_count().saturating_sub(clone_baseline);
        let download_delta = download_stats() - download_baseline;
        print_install_counts(added, download_delta.reused, download_delta.downloaded);
        Ok(())
    }

    pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
        // Prepare global package directory and package.json
        let package_path = prepare_global_package_json(npm_spec, prefix)
            .await
            .context("Failed to prepare global package.json")?;

        tracing::debug!("Installing global package: {npm_spec}");

        // Install dependencies (global install never omits)
        Self::install(ScriptPolicy::Run, &package_path, &HashSet::new())
            .await
            .context("Failed to install global package dependencies")?;

        // Create package info from path
        let package_info = PackageInfo::from_path(&package_path)
            .await
            .context("Failed to create package info from path")?;

        // Get global bin directory using the common helper
        let target_bin_dir =
            get_global_bin_dir(prefix).context("Failed to get global bin directory")?;

        // Link binary files to global
        tracing::debug!(
            "Linking binary files to global... {}",
            target_bin_dir.display()
        );
        package_info
            .link_to_global(&target_bin_dir)
            .await
            .context("Failed to link binary files to global")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name_from_path() {
        // Test extracting package name from a standard path
        assert_eq!(extract_package_name("node_modules/lodash"), "lodash");

        // Test extracting package name from a nested path
        assert_eq!(
            extract_package_name("node_modules/parent/node_modules/child"),
            "child"
        );

        // Test extracting package name from a scoped package path
        assert_eq!(
            extract_package_name("node_modules/@scope/package"),
            "@scope/package"
        );
    }

    #[test]
    fn test_should_omit_package() {
        use std::collections::HashSet;

        // Empty omit set should not omit anything
        let empty_omit: HashSet<OmitType> = HashSet::new();
        let dev_pkg = Package {
            dev: Some(true),
            ..Package::default()
        };
        assert!(!should_omit_package(&dev_pkg, &empty_omit));

        // Omit dev packages
        let mut omit_dev: HashSet<OmitType> = HashSet::new();
        omit_dev.insert(OmitType::Dev);

        let dev_pkg = Package {
            dev: Some(true),
            ..Package::default()
        };
        assert!(should_omit_package(&dev_pkg, &omit_dev));

        let prod_pkg = Package::default();
        assert!(!should_omit_package(&prod_pkg, &omit_dev));

        // Omit optional packages
        let mut omit_optional: HashSet<OmitType> = HashSet::new();
        omit_optional.insert(OmitType::Optional);

        let optional_pkg = Package {
            optional: Some(true),
            ..Package::default()
        };
        assert!(should_omit_package(&optional_pkg, &omit_optional));

        // Omit peer packages
        let mut omit_peer: HashSet<OmitType> = HashSet::new();
        omit_peer.insert(OmitType::Peer);

        let peer_pkg = Package {
            peer: Some(true),
            ..Package::default()
        };
        assert!(should_omit_package(&peer_pkg, &omit_peer));

        // devOptional: only omit when both dev and optional are omitted
        let dev_optional_pkg = Package {
            dev_optional: Some(true),
            ..Package::default()
        };

        // Only dev omitted - should NOT omit devOptional
        assert!(!should_omit_package(&dev_optional_pkg, &omit_dev));

        // Only optional omitted - should NOT omit devOptional
        assert!(!should_omit_package(&dev_optional_pkg, &omit_optional));

        // Both dev and optional omitted - should omit devOptional
        let mut omit_dev_optional: HashSet<OmitType> = HashSet::new();
        omit_dev_optional.insert(OmitType::Dev);
        omit_dev_optional.insert(OmitType::Optional);
        assert!(should_omit_package(&dev_optional_pkg, &omit_dev_optional));
    }

    #[test]
    fn test_is_optional_dependency() {
        // Test helper to verify is_optional detection logic
        // This mirrors the logic used in install_packages

        // Regular package - not optional
        let regular_pkg = Package::default();
        let is_optional =
            regular_pkg.optional == Some(true) || regular_pkg.dev_optional == Some(true);
        assert!(!is_optional, "Regular package should not be optional");

        // Optional package
        let optional_pkg = Package {
            optional: Some(true),
            ..Package::default()
        };
        let is_optional =
            optional_pkg.optional == Some(true) || optional_pkg.dev_optional == Some(true);
        assert!(is_optional, "Package with optional=true should be optional");

        // Dev optional package
        let dev_optional_pkg = Package {
            dev_optional: Some(true),
            ..Package::default()
        };
        let is_optional =
            dev_optional_pkg.optional == Some(true) || dev_optional_pkg.dev_optional == Some(true);
        assert!(
            is_optional,
            "Package with dev_optional=true should be optional"
        );

        // Package with optional=false explicitly
        let not_optional_pkg = Package {
            optional: Some(false),
            ..Package::default()
        };
        let is_optional =
            not_optional_pkg.optional == Some(true) || not_optional_pkg.dev_optional == Some(true);
        assert!(
            !is_optional,
            "Package with optional=false should not be optional"
        );
    }

    fn lock_pkg(name: &str, version: &str, resolved: &str) -> Package {
        Package {
            name: Some(name.to_string()),
            version: Some(version.to_string()),
            resolved: Some(resolved.to_string()),
            ..Package::default()
        }
    }

    #[test]
    fn fresh_lock_prefetches_only_proven_registry_specs() {
        let mut packages = HashMap::new();
        packages.insert(
            String::new(),
            Package {
                dependencies: Some(HashMap::from([
                    ("react".to_string(), "^18.2.0".to_string()),
                    (
                        "remote-tarball".to_string(),
                        "https://example.com/remote-tarball.tgz".to_string(),
                    ),
                    (
                        "file-tarball".to_string(),
                        "file:./file-tarball.tgz".to_string(),
                    ),
                ])),
                ..Package::default()
            },
        );
        packages.insert(
            "node_modules/react".to_string(),
            lock_pkg(
                "react",
                "18.2.0",
                "https://registry.npmjs.org/react/-/react-18.2.0.tgz",
            ),
        );
        packages.insert(
            "node_modules/remote-tarball".to_string(),
            lock_pkg(
                "remote-tarball",
                "1.0.0",
                "https://example.com/remote-tarball.tgz",
            ),
        );
        packages.insert(
            "node_modules/file-tarball".to_string(),
            lock_pkg("file-tarball", "1.0.0", "file:./file-tarball.tgz"),
        );

        let prefetches = collect_fresh_lock_registry_packages(&packages);

        assert_eq!(prefetches.len(), 1);
        assert_eq!(prefetches[0].path, "node_modules/react");
        assert_eq!(prefetches[0].name, "react");
        assert_eq!(prefetches[0].version, "18.2.0");
    }

    #[test]
    fn fresh_lock_prefetch_uses_physical_parent_for_nested_paths() {
        let mut packages = HashMap::new();
        packages.insert(String::new(), Package::default());
        packages.insert(
            "node_modules/@scope/parent".to_string(),
            Package {
                name: Some("@scope/parent".to_string()),
                version: Some("1.0.0".to_string()),
                dependencies: Some(HashMap::from([("child".to_string(), "~1.0.0".to_string())])),
                ..Package::default()
            },
        );
        packages.insert(
            "node_modules/@scope/parent/node_modules/child".to_string(),
            lock_pkg(
                "child",
                "1.0.1",
                "https://registry.npmjs.org/child/-/child-1.0.1.tgz",
            ),
        );

        let prefetches = collect_fresh_lock_registry_packages(&packages);

        assert_eq!(prefetches.len(), 1);
        assert_eq!(
            prefetches[0].path,
            "node_modules/@scope/parent/node_modules/child"
        );
        assert_eq!(prefetches[0].name, "child");
    }
}
