use crate::util::cli_enum::ScriptPolicy;
use anyhow::Context;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use crate::cmd::deps::build_deps;
use crate::fs;
use crate::helper::global_bin::get_global_bin_dir;
use crate::helper::lock::{
    Package, UpdatePackageJsonOptions, extract_package_name, group_by_depth, is_pkg_lock_outdated,
    prepare_global_package_json, update_package_json,
};
use crate::helper::workspace::init_project_root;
use crate::model::package::PackageInfo;
use crate::service::rebuild::RebuildService;
use crate::util::cli_enum::{OmitType, PackageAction, SaveType};
use crate::util::cloner::clone_count;
use crate::util::downloader::download_stats;
use crate::util::json::load_package_lock_json_from_path;
use crate::util::linker::link;
use crate::util::logger::{
    finish_progress_bar, inc_progress, log_progress, log_progress_lazy, print_install_counts,
    set_progress_length, start_progress_bar,
};
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};

use super::binary::update_package_binary;
use super::clean::clean_deps;

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

pub async fn install_packages(
    groups: &HashMap<usize, Vec<(String, Package)>>,
    cwd: &Path,
    omit: &HashSet<OmitType>,
) -> Result<()> {
    use crate::util::cloner::clone_package_once;

    // Surface the clean step in the spinner — it doesn't move `pos`, so
    // without a message the bar looks frozen on large trees.
    log_progress("validating node_modules");
    clean_deps(groups, cwd).await?;
    log_progress("linking packages");

    // Always process level-by-level to ensure parent directories exist before
    // children. Within each level, tasks run concurrently. The pipeline's
    // clone_worker may have already cloned some packages — clone_package_once
    // deduplicates via CLONE_CACHE so no double work occurs.
    let mut depths: Vec<_> = groups.keys().cloned().collect();
    depths.sort_unstable();

    for depth in depths.iter() {
        let mut clone_tasks: Vec<tokio::task::JoinHandle<Result<()>>> = Vec::new();

        if let Some(packages) = groups.get(depth) {
            for (path, package) in packages.iter() {
                // Skip packages based on omit config
                if should_omit_package(package, omit) {
                    inc_progress(1);
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
                            inc_progress(1);
                            continue;
                        }
                        link(Path::new(&resolved), Path::new(&path))
                            .await
                            .with_context(|| format!("Link failed: {resolved} -> {path}"))?;
                        inc_progress(1);
                        continue;
                    }

                    // skip when cpu or os is not compatible
                    if let Some(ref cpu) = package.cpu
                        && !is_cpu_compatible(cpu)
                    {
                        inc_progress(1);
                        continue;
                    }

                    if let Some(ref os) = package.os
                        && !is_os_compatible(os)
                    {
                        inc_progress(1);
                        continue;
                    }

                    let name = package.get_name(&path);
                    let version = package
                        .version
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("package {name} missing version"))?;
                    let cwd_clone = cwd.to_path_buf();
                    let target_path = cwd_clone.join(&path);

                    // Check if this is an optional dependency
                    let is_optional =
                        package.optional == Some(true) || package.dev_optional == Some(true);

                    let task = tokio::spawn(async move {
                        if let Err(e) =
                            clone_package_once(&name, &version, &resolved, &target_path).await
                        {
                            if is_optional {
                                tracing::warn!(
                                    "Optional dependency {name} failed (ignored): {e:#}"
                                );
                                inc_progress(1);
                                return Ok(());
                            }
                            return Err(e);
                        }
                        inc_progress(1);
                        log_progress_lazy(|| format!("{name} resolved"));
                        update_package_binary(&target_path, &name).await
                    });
                    clone_tasks.push(task);
                } else {
                    inc_progress(1);
                }
            }
        }

        for task in clone_tasks {
            task.await??;
        }
    }

    Ok(())
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

        let (package_lock, pipeline_handles) = if use_fresh_lock {
            let lock = load_package_lock_json_from_path(root_path).await?;
            (lock, None)
        } else {
            start_progress_bar();
            let resolve_start = Instant::now();
            let result = super::pipeline::resolve_with_pipeline(root_path).await?;
            finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));
            (result.package_lock, Some(result.handles))
        };

        let groups = group_by_depth(&package_lock.packages);

        if !package_lock.packages.is_empty() {
            start_progress_bar();
            set_progress_length(package_lock.packages.len() as u64);
        }

        let link_start = Instant::now();
        install_packages(&groups, root_path, omit)
            .await
            .context("Failed to install packages")?;

        // Wait for pipeline workers to complete (if any)
        if let Some(handles) = pipeline_handles {
            handles.await_completion().await;
            super::pipeline::print_pipeline_summary();
        }
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
}
