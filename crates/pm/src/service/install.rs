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
    Package, UpdatePackageJsonOptions, extract_package_name, group_by_depth, is_pkg_lock_outdated,
    resolve_package_spec_details, save_package_lock, update_package_json,
};
use crate::helper::ruborist_context::Context;
use crate::helper::workspace::init_project_root;
use crate::model::package::PackageInfo;
use crate::service::package::PackageService;
use crate::service::rebuild::RebuildService;
use crate::service::script::ScriptOutput;
use crate::util::cli_enum::{OmitType, PackageAction, ReifyMode, SaveType};
use crate::util::cloner::ClonePolicy;
use crate::util::install_progress;
use crate::util::json::load_package_lock_json_from_path;
use crate::util::linker::link;
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_progress, print_install_counts, start_progress_bar,
};
use crate::util::proxy_env::print_proxy_env_hint_once;
use utoo_ruborist::builder::DevDeps;
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};
use utoo_ruborist::manifest::PackageJson;
use utoo_ruborist::progress::PackageTarballInfo;

use super::binary::{requires_private_copy, update_package_binary};
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
    mode: ReifyMode,
) -> Result<()> {
    // Surface the clean step in the spinner — it doesn't move `pos`, so
    // without a message the bar looks frozen on large trees.
    log_progress("validating node_modules");
    clean_deps(groups, cwd).await?;
    reify_packages(groups, cwd, omit, scheduler, mode).await
}

async fn prepare_reify_target(target: &Path, mode: ReifyMode) -> Result<()> {
    if mode != ReifyMode::Force {
        return Ok(());
    }

    let Ok(metadata) = fs::symlink_metadata(target).await else {
        return Ok(());
    };
    let result = if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(target).await
    } else {
        fs::remove_dir_all(target).await
    };
    result.with_context(|| format!("Failed to force-replace {}", target.display()))
}

fn is_node_modules_path(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|component| component.as_os_str() == "node_modules")
}

/// Clone/link every package in `groups` into `<cwd>/node_modules`, level by
/// level, WITHOUT pruning extraneous entries (no `clean_deps`). Within each
/// level, tasks run concurrently.
async fn reify_packages(
    groups: &HashMap<usize, Vec<(String, Package)>>,
    cwd: &Path,
    omit: &HashSet<OmitType>,
    scheduler: &super::install_scheduler::InstallScheduler,
    mode: ReifyMode,
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
                    if mode == ReifyMode::Force && !is_node_modules_path(path) {
                        anyhow::bail!(
                            "Refusing to force-replace path outside node_modules: {path}"
                        );
                    }
                    let target_path = cwd.join(path);
                    prepare_reify_target(&target_path, mode).await?;
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
                    let scheduler = scheduler.clone();
                    // Packages with lifecycle scripts and packages patched by
                    // the binary-mirror pass are both mutated after cloning.
                    // Keep them private instead of hardlinking from cache.
                    let policy = if package.has_install_scripts() || requires_private_copy(&name) {
                        ClonePolicy::Private
                    } else {
                        ClonePolicy::Shared
                    };

                    // Check if this is an optional dependency
                    let is_optional = package.is_optional();

                    clone_tasks.push(async move {
                        if let Err(e) = scheduler
                            .ensure_clone(
                                name.clone(),
                                version,
                                resolved,
                                target_path.clone(),
                                policy,
                            )
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

fn global_install_root(prefix: Option<&str>, name: &str) -> Result<std::path::PathBuf> {
    Ok(get_global_package_dir(prefix)?.join(name))
}

impl InstallService {
    pub async fn update_packages(
        action: PackageAction,
        specs: &[&str],
        workspace: Option<String>,
        scripts: ScriptPolicy,
        save_type: SaveType,
        omit: &HashSet<OmitType>,
        output: ScriptOutput,
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

        Self::install(scripts, &root_path, omit, output)
            .await
            .context("Failed to install packages")?;

        Ok(())
    }

    pub async fn install(
        scripts: ScriptPolicy,
        root_path: &Path,
        omit: &HashSet<OmitType>,
        output: ScriptOutput,
    ) -> Result<()> {
        Self::install_with_mode(scripts, root_path, omit, ReifyMode::Incremental, output).await
    }

    pub async fn install_with_mode(
        scripts: ScriptPolicy,
        root_path: &Path,
        omit: &HashSet<OmitType>,
        mode: ReifyMode,
        output: ScriptOutput,
    ) -> Result<()> {
        print_proxy_env_hint_once();
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
        let install_result = install_packages(&groups, root_path, omit, &scheduler, mode)
            .await
            .context("Failed to install packages");

        let counts = scheduler_handle.shutdown().await;
        install_result?;
        let clone_elapsed = link_start.elapsed();
        finish_progress_bar("node_modules cloned", Some(clone_elapsed));

        RebuildService::rebuild(&package_lock, root_path, scripts, output).await?;

        print_install_counts(counts.cloned, counts.reused, counts.downloaded);
        Ok(())
    }

    /// Install one tool beneath an npm-style prefix.
    ///
    /// `utoo install -g` passes the user's global prefix; `utoo x` passes its
    /// per-name/version cache prefix. Both produce the same isolated layout:
    ///
    /// ```text
    /// <prefix>/
    /// ├── bin/<command> -> ../lib/node_modules/<name>/<bin-entry>
    /// └── lib/node_modules/<name>/
    ///     ├── package.json
    ///     └── node_modules/          # this tool's production dependencies
    /// ```
    ///
    /// The installed tool is the dependency-resolution root, so transitive
    /// packages cannot be hoisted beside other global tools. It participates in
    /// dependency lifecycle hooks (`preinstall`/`install`/`postinstall`) but not
    /// project-only hooks (`prepare`/`prepublish`) or root dev dependencies.
    pub async fn install_global_package(
        npm_spec: &str,
        prefix: Option<&str>,
        scripts: ScriptPolicy,
        output: ScriptOutput,
    ) -> Result<()> {
        print_proxy_env_hint_once();
        install_progress::start_install_run();
        let resolved = resolve_package_spec_details(npm_spec).await?;
        let global_node_modules = get_global_package_dir(prefix)?;
        let root_path = global_install_root(prefix, &resolved.name)?;
        fs::create_dir_all(&global_node_modules).await?;

        tracing::debug!(
            "Installing global package {} into {}",
            resolved.name,
            root_path.display()
        );

        let scheduler_handle = super::install_scheduler::InstallSchedulerHandle::start();
        let scheduler = scheduler_handle.scheduler();

        let install_result: Result<_> = async {
            // This entry point is overwrite-oriented. `ut x` skips it on a
            // cache hit; an explicit global install replaces only the requested
            // tool and never touches siblings in the shared node_modules.
            if fs::try_exists(&root_path).await.unwrap_or(false) {
                fs::remove_dir_all(&root_path)
                    .await
                    .with_context(|| format!("Failed to replace {}", root_path.display()))?;
            }

            scheduler
                .ensure_clone(
                    resolved.name.clone(),
                    resolved.version.clone(),
                    resolved.tarball_url.clone(),
                    root_path.clone(),
                    ClonePolicy::Private,
                )
                .await
                .context("Failed to materialize global package")?;
            update_package_binary(&root_path, &resolved.name).await?;

            let mut pkg: PackageJson = crate::util::json::load_package_json(&root_path)
                .await
                .context("Failed to load installed global tool")?;
            // Published packages are installed as dependencies, never as
            // workspace roots. Keep package.json intact; only the in-memory
            // resolution view suppresses workspace discovery.
            pkg.workspaces = None;

            // Resolve production dependencies with the package itself as the
            // real root. Resolver paths therefore land below
            // `<prefix>/lib/node_modules/<name>/node_modules`, not beside it.
            start_progress_bar();
            let resolve_start = Instant::now();
            let mut options =
                Context::install_deps_options(root_path.clone(), scheduler.clone()).await;
            // A package tarball may contain its publisher's lockfile. It does
            // not govern consumers, so never seed a global install from it.
            options.baseline = None;
            let lock = utoo_ruborist::service::build_deps_with_root_dev_deps(
                options,
                pkg,
                DevDeps::Exclude,
            )
            .await
            .context("Failed to resolve global package")?;
            finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));

            // The root was freshly materialized above. Reify only this tool's
            // lock beneath its own node_modules; sibling global tools are
            // outside `root_path` and cannot be pruned or overwritten.
            let groups = group_by_depth(&lock.packages);
            if !lock.packages.is_empty() {
                start_progress_bar();
                PROGRESS_BAR.set_length(lock.packages.len() as u64);
            }
            let link_start = Instant::now();
            reify_packages(
                &groups,
                &root_path,
                &HashSet::new(),
                &scheduler,
                ReifyMode::Incremental,
            )
            .await
            .context("Failed to install global package")?;
            finish_progress_bar("node_modules cloned", Some(link_start.elapsed()));
            Ok(lock)
        }
        .await;
        let counts = scheduler_handle.shutdown().await;
        let lock = install_result?;

        // Dependency lifecycle only: the shared queue knows only
        // preinstall/install/postinstall. Add the root package explicitly
        // because roots are not dependency entries in the lock, but leave its
        // bins for the prefix-level link step below.
        let package_info = PackageInfo::from_path(&root_path)
            .await
            .context("Failed to load installed global tool")?;
        let mut root_lifecycle = package_info.clone();
        root_lifecycle.bin_files.clear();
        let mut packages =
            PackageService::collect_packages_from_lock(&lock, &root_path, scripts).await?;
        packages.push((root_lifecycle, false));
        if !packages.is_empty() {
            let queues = PackageService::create_execution_queues_with_options(packages, scripts)?;
            PackageService::execute_queues_with_options(queues, scripts, output).await?;
        }

        // Link the tool's own bin into the global bin dir.
        let target_bin_dir =
            get_global_bin_dir(prefix).context("Failed to get global bin directory")?;
        package_info
            .link_to_global(&target_bin_dir)
            .await
            .context("Failed to link binary files to global")?;

        print_install_counts(counts.cloned, counts.reused, counts.downloaded);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::package::{LifecycleScripts, PackageInfo};
    use crate::util::platform_const::GLOBAL_NODE_MODULES;
    use tempfile::tempdir;

    #[test]
    fn global_install_root_is_the_package_isolation_boundary() {
        let temp = tempdir().unwrap();
        let prefix = temp.path().to_string_lossy();
        let root = global_install_root(Some(&prefix), "@scope/tool").unwrap();

        assert_eq!(
            root,
            temp.path()
                .join(GLOBAL_NODE_MODULES)
                .join("@scope")
                .join("tool")
        );
        assert_eq!(
            root.join("node_modules"),
            temp.path()
                .join(GLOBAL_NODE_MODULES)
                .join("@scope")
                .join("tool")
                .join("node_modules")
        );
    }

    #[test]
    fn global_tool_queues_dependency_lifecycle_only() {
        let package = PackageInfo {
            path: "/global/tool".into(),
            bin_files: vec![],
            scripts: Default::default(),
            lifecycle_scripts: LifecycleScripts::from_scripts(&HashMap::from([
                ("preinstall".into(), "echo preinstall".into()),
                ("install".into(), "echo install".into()),
                ("postinstall".into(), "echo postinstall".into()),
                ("prepare".into(), "echo prepare".into()),
            ])),
            name: "tool".into(),
        };

        let queues = PackageService::create_execution_queues_with_options(
            vec![(package, false)],
            ScriptPolicy::Run,
        )
        .unwrap();

        assert_eq!(queues.preinstall.len(), 1);
        assert_eq!(queues.install.len(), 1);
        assert_eq!(queues.postinstall.len(), 1);
        assert!(queues.bin_linking.is_empty());
    }

    #[tokio::test]
    async fn force_reify_removes_existing_package_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("node_modules").join("pkg");
        fs::create_dir_all(&target).await.unwrap();
        fs::write(
            target.join("package.json"),
            br#"{"name":"pkg","version":"1.0.0"}"#,
        )
        .await
        .unwrap();
        fs::write(target.join("locally-modified.js"), b"modified")
            .await
            .unwrap();

        prepare_reify_target(&target, ReifyMode::Force)
            .await
            .unwrap();

        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn force_reify_removes_link_without_touching_source() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("workspace");
        let target = temp.path().join("node_modules").join("workspace");
        fs::create_dir_all(&source).await.unwrap();
        fs::create_dir_all(target.parent().unwrap()).await.unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();

        prepare_reify_target(&target, ReifyMode::Force)
            .await
            .unwrap();

        assert!(source.exists());
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn incremental_reify_preserves_existing_package_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("node_modules").join("pkg");
        fs::create_dir_all(&target).await.unwrap();
        fs::write(target.join("locally-modified.js"), b"modified")
            .await
            .unwrap();

        prepare_reify_target(&target, ReifyMode::Incremental)
            .await
            .unwrap();

        assert!(target.join("locally-modified.js").exists());
    }

    #[test]
    fn force_reify_is_limited_to_node_modules_paths() {
        assert!(!is_node_modules_path(""));
        assert!(!is_node_modules_path("packages/app"));
        assert!(is_node_modules_path("node_modules/pkg"));
        assert!(is_node_modules_path(
            "node_modules/parent/node_modules/child"
        ));
    }

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
