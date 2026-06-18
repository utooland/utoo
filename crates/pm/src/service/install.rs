use crate::util::cli_enum::ScriptPolicy;
use anyhow::{Context as _, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use crate::cmd::deps::build_deps;
use crate::fs;
use crate::helper::global_bin::{get_global_bin_dir, get_global_package_dir};
use crate::helper::lock::{
    Package, UpdatePackageJsonOptions, extract_package_name, format_save_spec, group_by_depth,
    is_pkg_lock_outdated, resolve_package_spec, save_package_lock, update_package_json,
};
use crate::helper::ruborist_context::Context;
use crate::helper::workspace::init_project_root;
use crate::model::package::PackageInfo;
use crate::service::package::PackageService;
use crate::service::rebuild::RebuildService;
use crate::util::cli_enum::{OmitType, PackageAction, SaveType};
use crate::util::install_progress;
use crate::util::json::load_package_lock_json_from_path;
use crate::util::linker::link;
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_progress, print_install_counts, start_progress_bar,
};
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};
use utoo_ruborist::progress::PackageTarballInfo;

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

/// Disposition of a lock entry in the install pipeline.
///
/// Derived in one place so the lockfile prefetch seed and `reify_packages`
/// cannot drift — an earlier hand-rolled prefetch filter did, and
/// over-downloaded gigabytes of incompatible binaries. (The platform filter
/// lives in the shared `prefetch_tarball` gate for the same reason.)
enum LockEntryAction {
    /// Omitted by --omit config: not installed at all.
    Skip,
    /// Workspace `link:` entry: symlinked, never cloned or downloaded.
    Link,
    /// Regular entry: cloned via the scheduler (and thus prefetchable).
    Clone,
}

fn classify_lock_entry(package: &Package, omit: &HashSet<OmitType>) -> LockEntryAction {
    if should_omit_package(package, omit) {
        LockEntryAction::Skip
    } else if package.link.is_some() {
        LockEntryAction::Link
    } else {
        LockEntryAction::Clone
    }
}

async fn install_packages(
    groups: &HashMap<usize, Vec<(String, Package)>>,
    cwd: &Path,
    omit: &HashSet<OmitType>,
    scheduler: &super::install_scheduler::InstallScheduler,
) -> Result<()> {
    // Surface the clean step in the spinner — it doesn't move `pos`, so
    // without a message the bar looks frozen on large trees.
    log_progress("validating node_modules");
    clean_deps(groups, cwd).await?;
    reify_packages(groups, cwd, omit, scheduler).await
}

/// Clone/link every package in `groups` into `<cwd>/node_modules`, level by
/// level, WITHOUT pruning extraneous entries (no `clean_deps`). Within each
/// level, tasks run concurrently. Global installs (`utoo install -g`, `utoo x`)
/// reify additively into a shared global `node_modules` so previously-installed
/// tools survive — and there is no synthetic root `package.json` on disk for
/// `clean_deps` / `find_workspaces` to read.
async fn reify_packages(
    groups: &HashMap<usize, Vec<(String, Package)>>,
    cwd: &Path,
    omit: &HashSet<OmitType>,
    scheduler: &super::install_scheduler::InstallScheduler,
) -> Result<()> {
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
                let action = classify_lock_entry(package, omit);
                if matches!(action, LockEntryAction::Skip) {
                    PROGRESS_BAR.inc(1);
                    continue;
                }
                // No clones here: the spawned task only captures the owned
                // name/version/resolved/target_path it actually needs, so
                // skipped/linked entries never pay for a LockPackage copy.
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
                    if matches!(action, LockEntryAction::Link) {
                        let link_name = extract_package_name(path);
                        if link_name.is_empty() {
                            PROGRESS_BAR.inc(1);
                            continue;
                        }
                        link(Path::new(&resolved), Path::new(path))
                            .await
                            .with_context(|| format!("Link failed: {resolved} -> {path}"))?;
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    // skip when cpu or os is not compatible
                    if let Some(ref cpu) = package.cpu
                        && !is_cpu_compatible(cpu)
                    {
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    if let Some(ref os) = package.os
                        && !is_os_compatible(os)
                    {
                        PROGRESS_BAR.inc(1);
                        continue;
                    }

                    let name = package.get_name(path);
                    let version = package
                        .version
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("package {name} missing version"))?;
                    let target_path = cwd.join(path);
                    let scheduler = scheduler.clone();

                    // Check if this is an optional dependency
                    let is_optional = package.is_optional();

                    clone_tasks.push(async move {
                        if let Err(e) = scheduler
                            .ensure_clone(name.clone(), version, resolved, target_path.clone())
                            .await
                        {
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
    let (resolved_root, pkg) =
        utoo_ruborist::service::read_root_manifest(root_path, Context::glob()).await?;
    let options = Context::install_deps_options(resolved_root.clone(), scheduler).await;
    let lock = utoo_ruborist::service::build_deps(options, pkg).await?;

    // Persist at the resolved workspace root the lock was built against (not the
    // caller's possibly-nested `root_path`), so the lockfile and its root-relative
    // `resolved` paths stay consistent with where it lives.
    save_package_lock(&resolved_root, &lock).await?;

    Ok(lock)
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
        install_progress::start_install_run();
        let lock_path = root_path.join("package-lock.json");
        // Treat a failing freshness check as stale: regenerate rather than
        // install from a lockfile we couldn't validate. `is_pkg_lock_outdated`
        // itself emits a `tracing::warn` with the specific mismatch reason.
        let use_fresh_lock = fs::try_exists(&lock_path).await.unwrap_or(false)
            && !is_pkg_lock_outdated(root_path).await.unwrap_or(true);
        let scheduler_handle = super::install_scheduler::InstallSchedulerHandle::start();
        let scheduler = scheduler_handle.scheduler();

        let (package_lock, events_prefetched) = if use_fresh_lock {
            let lock = match load_package_lock_json_from_path(root_path).await {
                Ok(lock) => lock,
                Err(e) => {
                    scheduler_handle.shutdown().await;
                    return Err(e);
                }
            };
            (lock, false)
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
            (lock, true)
        };

        // Installing from an existing lockfile fires no resolver events, so the
        // download pipeline would otherwise be driven only by the depth-by-depth
        // `ensure_clone` pass (each level awaited before the next) — serializing
        // tarball downloads behind the clone order. Seed every package's
        // download up front so the network runs ahead of, and overlaps with, the
        // level-by-level clone — the same head start the fresh-resolve path gets
        // from `PackageResolved` events. Non-registry specs (file/git/link) are
        // filtered inside `prefetch_download`; the scheduler dedupes against the
        // authoritative `ensure_clone`.
        if !events_prefetched {
            for (path, package) in &package_lock.packages {
                // Seed only what `install_packages` will actually clone — the
                // same `classify_lock_entry` it consumes, so the two passes
                // can't drift. The platform filter lives in the shared
                // `prefetch_tarball` gate (same one the resolver event path
                // uses) for the same reason.
                if !matches!(classify_lock_entry(package, omit), LockEntryAction::Clone) {
                    continue;
                }
                if let (Some(version), Some(resolved)) =
                    (package.version.as_deref(), package.resolved.as_deref())
                {
                    let name = package.get_name(path);
                    scheduler.prefetch_tarball(&PackageTarballInfo {
                        name: &name,
                        version,
                        tarball_url: Some(resolved),
                        integrity: None,
                        os: package.os.as_ref(),
                        cpu: package.cpu.as_ref(),
                    });
                }
            }
        }

        let groups = group_by_depth(&package_lock.packages);

        if !package_lock.packages.is_empty() {
            start_progress_bar();
            PROGRESS_BAR.set_length(package_lock.packages.len() as u64);
        }

        let link_start = Instant::now();
        let install_result = install_packages(&groups, root_path, omit, &scheduler)
            .await
            .context("Failed to install packages");

        let counts = scheduler_handle.shutdown().await;
        install_result?;
        let clone_elapsed = link_start.elapsed();
        finish_progress_bar("node_modules cloned", Some(clone_elapsed));

        RebuildService::rebuild(&package_lock, root_path, scripts).await?;

        print_install_counts(
            counts.cloned,
            counts.reused,
            counts.downloaded,
            Some(clone_elapsed),
        );
        Ok(())
    }

    /// Install a package globally (`utoo install -g`, `utoo x`).
    ///
    /// The tool is installed as a **production dependency** of an in-memory
    /// synthetic root — never as a root project — so it runs the install
    /// lifecycle (`preinstall`/`install`/`postinstall` + bin) but never
    /// `prepare`/`prepublish`, and its `devDependencies` are not installed
    /// (matching `npm install -g` and bun). No wrapper `package.json` is written:
    /// the global `node_modules` is the source of truth, reified **additively**
    /// so previously-installed globals survive.
    pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
        install_progress::start_install_run();
        let (name, resolved_version, version_spec) = resolve_package_spec(npm_spec).await?;
        // Resolvable spec for the synthetic dependency: registry ranges pinned to
        // the resolved version; git/file/url specs kept as-is.
        let dep_spec = format_save_spec(&version_spec, &resolved_version);

        // Shared global `node_modules` base (`<prefix>/lib/node_modules`); reify
        // from its parent so the tool lands at `<root>/node_modules/<name>`.
        let global_node_modules = get_global_package_dir(prefix)?;
        let root_path = global_node_modules
            .parent()
            .context("global node_modules has no parent directory")?
            .to_path_buf();
        fs::create_dir_all(&root_path).await?;

        // Synthetic private root: `{ private, dependencies: { <name>: <spec> } }`.
        // Lives only in memory — fed straight to the resolver.
        let mut pkg = utoo_ruborist::manifest::PackageJson::new("utoo-global", "0.0.0");
        pkg.private = Some(true);
        pkg.dependencies = Some(HashMap::from([(name.clone(), dep_spec)]));

        tracing::debug!(
            "Installing global package {name} into {}",
            root_path.display()
        );

        // Production install: never pull devDependencies.
        let omit = HashSet::from([OmitType::Dev]);

        let scheduler_handle = super::install_scheduler::InstallSchedulerHandle::start();
        let scheduler = scheduler_handle.scheduler();

        // Resolve the tool + its prod deps from the synthetic root. Resolver
        // events drive the download pipeline (same head start a cold install gets).
        start_progress_bar();
        let resolve_start = Instant::now();
        let options = Context::install_deps_options(root_path.clone(), scheduler.clone()).await;
        let lock = match utoo_ruborist::service::build_deps(options, pkg).await {
            Ok(lock) => lock,
            Err(e) => {
                scheduler_handle.shutdown().await;
                return Err(e).context("Failed to resolve global package");
            }
        };
        finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));

        // Reify ADDITIVELY (no clean_deps) into the shared global node_modules.
        let groups = group_by_depth(&lock.packages);
        if !lock.packages.is_empty() {
            start_progress_bar();
            PROGRESS_BAR.set_length(lock.packages.len() as u64);
        }
        let link_start = Instant::now();
        let reify = reify_packages(&groups, &root_path, &omit, &scheduler).await;
        let counts = scheduler_handle.shutdown().await;
        reify.context("Failed to install global package")?;
        let clone_elapsed = link_start.elapsed();
        finish_progress_bar("node_modules cloned", Some(clone_elapsed));

        // Dependency lifecycle only — preinstall/install/postinstall + bin
        // linking for the tool and its deps. No project/workspace hooks, so no
        // `prepare`/`prepublish`, and no disk root `package.json` is required.
        let packages =
            PackageService::collect_packages_from_lock(&lock, &root_path, ScriptPolicy::Run)
                .await?;
        if !packages.is_empty() {
            let queues =
                PackageService::create_execution_queues_with_options(packages, ScriptPolicy::Run)?;
            PackageService::execute_queues_with_options(queues, ScriptPolicy::Run).await?;
        }

        // Link the tool's own bin into the global bin dir.
        let tool_dir = global_node_modules.join(&name);
        let package_info = PackageInfo::from_path(&tool_dir)
            .await
            .context("Failed to load installed global tool")?;
        let target_bin_dir =
            get_global_bin_dir(prefix).context("Failed to get global bin directory")?;
        package_info
            .link_to_global(&target_bin_dir)
            .await
            .context("Failed to link binary files to global")?;

        print_install_counts(
            counts.cloned,
            counts.reused,
            counts.downloaded,
            Some(clone_elapsed),
        );
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
        let is_optional = regular_pkg.is_optional();
        assert!(!is_optional, "Regular package should not be optional");

        // Optional package
        let optional_pkg = Package {
            optional: Some(true),
            ..Package::default()
        };
        let is_optional = optional_pkg.is_optional();
        assert!(is_optional, "Package with optional=true should be optional");

        // Dev optional package
        let dev_optional_pkg = Package {
            dev_optional: Some(true),
            ..Package::default()
        };
        let is_optional = dev_optional_pkg.is_optional();
        assert!(
            is_optional,
            "Package with dev_optional=true should be optional"
        );

        // Package with optional=false explicitly
        let not_optional_pkg = Package {
            optional: Some(false),
            ..Package::default()
        };
        let is_optional = not_optional_pkg.is_optional();
        assert!(
            !is_optional,
            "Package with optional=false should not be optional"
        );
    }
}
