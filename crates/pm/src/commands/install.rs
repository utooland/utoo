//! Dependency installation commands.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::Result;
use clap::Args;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::spec::PackageSpec;

pub use crate::helper::migrate::FromPm;
use crate::helper::migrate::migrate_from_pnpm;
use crate::helper::workspace::init_project_root;
use crate::model::cli_output::{
    DependencyOperation, DependencyScope, DependencySummary, InstallResult, PackageVersion,
};
use crate::service::install::InstallService;
use crate::service::script::ScriptOutput;
use crate::util::cli_enum::{
    InstallScope, OmitType, PackageAction, ReifyMode, SaveType, ScriptPolicy,
};
use crate::util::format_print::{pluralized_package_count, print_migrate_result};
use crate::util::install_progress::DownloadBaseline;
use crate::util::invocation;
use crate::util::json::load_package_lock_json_from_path;
use crate::util::logger::log_time_end;
use crate::util::presenter::emit;
use crate::util::user_config::{get_omit, resolve_global_prefix, set_install_scope, set_omit};

/// Arguments for the `install` command, parsed by clap at the CLI boundary.
#[derive(Args)]
pub struct InstallArgs {
    /// Package specifications (e.g. "lodash@4.17.21" "react@18.0.0")
    pub specs: Vec<String>,

    /// Workspace to install in
    #[arg(short, long)]
    pub workspace: Option<String>,

    /// Skip running dependency scripts
    #[arg(long)]
    pub ignore_scripts: bool,

    /// Save as production dependency (default behavior)
    #[arg(long, short = 'S', default_value_t = true)]
    pub save: bool,

    /// Save as dev dependency
    #[arg(long, short = 'D')]
    pub save_dev: bool,

    /// Save as peer dependency
    #[arg(long)]
    pub save_peer: bool,

    /// Save as optional dependency
    #[arg(long, short = 'O')]
    pub save_optional: bool,

    /// Install package globally
    #[arg(short, long)]
    pub global: bool,

    #[arg(short, long)]
    pub prefix: Option<String>,

    /// Only install production dependencies (omit dev and optional)
    #[arg(long)]
    pub production: bool,

    /// Dependency types to omit
    #[arg(long, value_delimiter = ',')]
    pub omit: Vec<OmitType>,

    /// Migrate from another package manager before installing
    #[arg(long)]
    pub from: Option<FromPm>,
}

pub use InstallArgs as Options;

/// Entry point for the `install` command.
///
/// Folds `--production` and `--legacy-peer-deps` into the omit set, then
/// installs either the given specs (globally or locally) or the whole project.
pub async fn run(args: InstallArgs, legacy_peer_deps: Option<bool>) -> Result<()> {
    let download_baseline = DownloadBaseline::capture();
    let machine = invocation::json();
    let requested = args.specs.clone();
    let scope = InstallScope::from(args.global);
    let workspace = (scope == InstallScope::Local)
        .then(|| args.workspace.clone())
        .flatten();
    let root_path = if machine && scope == InstallScope::Local {
        let cwd = std::env::current_dir()?;
        Some(init_project_root(&cwd).await?)
    } else {
        None
    };
    let before = load_lock_snapshot(root_path.as_deref()).await;
    // Build omit config: production = omit dev + optional
    let mut omit_set: HashSet<OmitType> = args.omit.into_iter().collect();
    if args.production {
        omit_set.insert(OmitType::Dev);
        omit_set.insert(OmitType::Optional);
    }
    // legacy_peer_deps means omit peer
    if legacy_peer_deps == Some(true) {
        omit_set.insert(OmitType::Peer);
    }
    set_omit(omit_set);

    if scope == InstallScope::Global {
        set_install_scope(scope);
    }

    if args.specs.is_empty() {
        install_cwd_inner(ScriptPolicy::from(args.ignore_scripts)).await?;
    } else if scope == InstallScope::Global {
        // For global installs, process packages one by one
        for spec in args.specs.iter() {
            global(spec, args.prefix.as_deref()).await?;
        }
        log_time_end(&pluralized_package_count(args.specs.len(), "installed"));
    } else {
        let save_type = SaveType::from_flags(args.save_dev, args.save_peer, args.save_optional);
        let spec_refs: Vec<&str> = args.specs.iter().map(|s| s.as_str()).collect();
        update_packages(
            PackageAction::Add,
            &spec_refs,
            args.workspace,
            ScriptPolicy::from(args.ignore_scripts),
            save_type,
        )
        .await?;
        // Log install result with correct singular/plural form in one line
        log_time_end(&pluralized_package_count(args.specs.len(), "installed"));
    }
    if !machine {
        return Ok(());
    }
    let after = load_lock_snapshot(root_path.as_deref()).await;
    let resolved = if let Some(lock) = after.as_ref() {
        direct_packages(lock, &requested)
    } else {
        global_packages(&requested, args.prefix.as_deref()).await
    };
    let output = InstallResult {
        operation: if requested.is_empty() {
            DependencyOperation::Install
        } else {
            DependencyOperation::Add
        },
        scope: dependency_scope(scope),
        workspace,
        requested,
        resolved,
        summary: dependency_summary(
            before.as_ref(),
            after.as_ref(),
            download_baseline.downloaded_bytes(),
        ),
    };
    emit("install", &output, || Ok(()))
}

/// Install all dependencies of the project containing the current directory.
/// Shared by bare `utoo` and `utoo install` without specs.
pub async fn current_project(scripts: ScriptPolicy) -> Result<()> {
    let download_baseline = DownloadBaseline::capture();
    let machine = invocation::json();
    let root_path = if machine {
        let cwd = std::env::current_dir()?;
        Some(init_project_root(&cwd).await?)
    } else {
        None
    };
    let before = load_lock_snapshot(root_path.as_deref()).await;
    install_cwd_inner(scripts).await?;
    if !machine {
        return Ok(());
    }
    let after = load_lock_snapshot(root_path.as_deref()).await;
    let output = InstallResult {
        operation: DependencyOperation::Install,
        scope: DependencyScope::Local,
        workspace: None,
        requested: Vec::new(),
        resolved: after
            .as_ref()
            .map(|lock| direct_packages(lock, &[]))
            .unwrap_or_default(),
        summary: dependency_summary(
            before.as_ref(),
            after.as_ref(),
            download_baseline.downloaded_bytes(),
        ),
    };
    emit("install", &output, || Ok(()))
}

async fn install_cwd_inner(scripts: ScriptPolicy) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root_path = init_project_root(&cwd).await?;
    project(&root_path, scripts).await?;
    log_time_end("All packages installed");
    Ok(())
}

fn dependency_scope(scope: InstallScope) -> DependencyScope {
    if scope.is_global() {
        DependencyScope::Global
    } else {
        DependencyScope::Local
    }
}

pub(super) async fn load_lock_snapshot(root_path: Option<&Path>) -> Option<PackageLock> {
    let root_path = root_path?;
    load_package_lock_json_from_path(root_path).await.ok()
}

fn package_snapshot(lock: &PackageLock) -> BTreeMap<&str, (&str, &str)> {
    lock.packages
        .iter()
        .filter(|(path, _)| !path.is_empty())
        .map(|(path, package)| {
            (
                path.as_str(),
                (
                    package.name.as_deref().unwrap_or("unknown"),
                    package.version.as_deref().unwrap_or("unknown"),
                ),
            )
        })
        .collect()
}

pub(super) fn dependency_summary(
    before: Option<&PackageLock>,
    after: Option<&PackageLock>,
    downloaded_bytes: u64,
) -> DependencySummary {
    let before = before.map(package_snapshot).unwrap_or_default();
    let after = after.map(package_snapshot).unwrap_or_default();
    let added = after
        .keys()
        .filter(|path| !before.contains_key(**path))
        .count() as u64;
    let removed = before
        .keys()
        .filter(|path| !after.contains_key(**path))
        .count() as u64;
    let changed = after
        .iter()
        .filter(|(path, package)| before.get(**path).is_some_and(|old| old != *package))
        .count() as u64;
    let reused = after
        .len()
        .saturating_sub(added as usize + changed as usize) as u64;
    DependencySummary {
        added,
        removed,
        changed,
        reused,
        downloaded_bytes,
    }
}

pub(super) fn direct_packages(lock: &PackageLock, requested: &[String]) -> Vec<PackageVersion> {
    let requested_names: HashSet<String> = requested
        .iter()
        .filter_map(|spec| match PackageSpec::from(spec.as_str()) {
            PackageSpec::Registry { name, .. } => Some(name),
            _ => None,
        })
        .collect();
    let direct_names: HashSet<&str> = lock
        .packages
        .get("")
        .into_iter()
        .flat_map(|root| {
            [
                root.dependencies.as_ref(),
                root.dev_dependencies.as_ref(),
                root.peer_dependencies.as_ref(),
                root.optional_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            .flat_map(|dependencies| dependencies.keys().map(String::as_str))
        })
        .collect();
    let mut packages = lock
        .packages
        .iter()
        .filter_map(|(path, package)| {
            let name = package
                .name
                .as_deref()
                .or_else(|| utoo_ruborist::lock::LockPackage::path_to_pkg_name(path))?;
            let is_direct = direct_names.contains(name)
                || (!requested_names.is_empty() && requested_names.contains(name));
            if !is_direct || (!requested_names.is_empty() && !requested_names.contains(name)) {
                return None;
            }
            Some(PackageVersion {
                name: name.to_string(),
                version: package
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            })
        })
        .collect::<Vec<_>>();
    packages.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    packages.dedup_by(|a, b| a.name == b.name);
    packages
}

async fn global_packages(requested: &[String], prefix: Option<&str>) -> Vec<PackageVersion> {
    let prefix = resolve_global_prefix(prefix).await;
    let Ok(root) = crate::helper::global_bin::get_global_package_dir(prefix.as_deref()) else {
        return Vec::new();
    };
    let mut packages = Vec::new();
    for spec in requested {
        let PackageSpec::Registry { name, .. } = PackageSpec::from(spec.as_str()) else {
            continue;
        };
        if let Ok(package) = crate::util::json::load_package_json::<
            utoo_ruborist::manifest::PackageJson,
        >(&root.join(&name))
        .await
        {
            packages.push(PackageVersion {
                name: package.name,
                version: package.version,
            });
        }
    }
    packages.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Run the `--from <pm>` migration pre-step.
///
/// Must run early — before the merged config is first loaded (and cached) by
/// `init_registry` — so the generated `.utoo.toml` is picked up.
pub async fn migrate_from(from: Option<FromPm>) -> Result<()> {
    if from == Some(FromPm::Pnpm) {
        let cwd = std::env::current_dir()?;
        let root_path = init_project_root(&cwd).await?;
        let result = migrate_from_pnpm(&root_path).await?;
        if !invocation::json() {
            print_migrate_result(&result)?;
        }
    }
    Ok(())
}

pub(super) async fn update_packages(
    action: PackageAction,
    specs: &[&str],
    workspace: Option<String>,
    scripts: ScriptPolicy,
    save_type: SaveType,
) -> Result<()> {
    let omit = get_omit();
    InstallService::update_packages(
        action,
        specs,
        workspace,
        scripts,
        save_type,
        &omit,
        script_output(),
    )
    .await
}

pub async fn project(root_path: &Path, scripts: ScriptPolicy) -> Result<()> {
    install_with_mode(scripts, root_path, ReifyMode::Incremental).await
}

pub(super) async fn install_with_mode(
    scripts: ScriptPolicy,
    root_path: &Path,
    mode: ReifyMode,
) -> Result<()> {
    let omit = get_omit();
    InstallService::install_with_mode(scripts, root_path, &omit, mode, script_output()).await
}

pub async fn global(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
    // Parameter validation
    if npm_spec.trim().is_empty() {
        anyhow::bail!("Package specification cannot be empty");
    }

    // Resolve the effective prefix: CLI flag > UTOO_PREFIX env > config.
    let prefix = resolve_global_prefix(prefix).await;

    // Dispatch to service
    InstallService::install_global_package(npm_spec, prefix.as_deref(), script_output()).await
}

fn script_output() -> ScriptOutput {
    if invocation::json() {
        ScriptOutput::Machine
    } else {
        ScriptOutput::Verbose
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_install_global_package_empty_spec() {
        // Test installing with empty package spec
        let result = global("", None).await;
        assert!(result.is_err(), "Should fail with empty package spec");

        let result = global("   ", None).await;
        assert!(
            result.is_err(),
            "Should fail with whitespace-only package spec"
        );
    }

    #[tokio::test]
    async fn test_update_packages_empty_specs() {
        // Test update with empty specs (the service layer rejects them)
        let result = update_packages(
            PackageAction::Add,
            &[],
            None,
            ScriptPolicy::Run,
            SaveType::Prod,
        )
        .await;
        assert!(result.is_err(), "Should fail with empty specs");
    }
}
