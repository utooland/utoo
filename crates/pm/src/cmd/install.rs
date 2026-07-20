use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::helper::migrate::{FromPm, migrate_from_pnpm};
use crate::helper::workspace::init_project_root;
use crate::service::install::InstallService;
use crate::util::cli_enum::{OmitType, PackageAction, SaveType, ScriptPolicy};
use crate::util::format_print::pluralized_package_count;
use crate::util::logger::log_time_end;
use crate::util::user_config::{
    InstallScope, get_omit, resolve_global_prefix, set_install_scope, set_omit,
};

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

/// Entry point for the `install` command.
///
/// Folds `--production` and `--legacy-peer-deps` into the omit set, then
/// installs either the given specs (globally or locally) or the whole project.
pub async fn run(args: InstallArgs, legacy_peer_deps: Option<bool>) -> Result<()> {
    let requested = args.specs.clone();
    let global = args.global;
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

    if args.global {
        set_install_scope(InstallScope::Global);
    }

    if args.specs.is_empty() {
        install_cwd_inner(ScriptPolicy::from(args.ignore_scripts)).await?;
    } else if args.global {
        // For global installs, process packages one by one
        for spec in args.specs.iter() {
            install_global_package(spec, args.prefix.as_deref()).await?;
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
    let operation = if requested.is_empty() { "sync" } else { "add" };
    emit_install_result("install", operation, &requested, global)
}

/// Entry point for the `uninstall` command.
pub async fn uninstall(
    specs: Vec<String>,
    workspace: Option<String>,
    scripts: ScriptPolicy,
) -> Result<()> {
    if specs.is_empty() {
        anyhow::bail!("Package specification is required for uninstall");
    }

    let spec_refs: Vec<&str> = specs.iter().map(|s| s.as_str()).collect();
    update_packages(
        PackageAction::Remove,
        &spec_refs,
        workspace,
        scripts,
        SaveType::Prod,
    )
    .await?;
    log_time_end(&pluralized_package_count(specs.len(), "uninstalled"));
    Ok(())
}

/// Install all dependencies of the project containing the current directory.
/// Shared by bare `utoo` and `utoo install` without specs.
pub async fn install_cwd(scripts: ScriptPolicy) -> Result<()> {
    install_cwd_inner(scripts).await?;
    emit_install_result("install", "sync", &[], false)
}

async fn install_cwd_inner(scripts: ScriptPolicy) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root_path = init_project_root(&cwd).await?;
    install(scripts, &root_path).await?;
    log_time_end("All packages installed");
    Ok(())
}

fn emit_install_result(
    command: &str,
    operation: &str,
    packages: &[String],
    global: bool,
) -> Result<()> {
    let output = InstallOutput {
        operation,
        changed: true,
        packages,
        global,
        downloaded_bytes: crate::util::install_progress::downloaded_bytes(),
    };
    crate::util::presenter::emit(command, &output, || Ok(()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallOutput<'a> {
    operation: &'a str,
    changed: bool,
    packages: &'a [String],
    global: bool,
    downloaded_bytes: u64,
}

/// Run the `--from <pm>` migration pre-step.
///
/// Must run early — before the merged config is first loaded (and cached) by
/// `init_registry` — so the generated `.utoo.toml` is picked up.
pub async fn migrate_from(from: Option<FromPm>) -> Result<()> {
    if from == Some(FromPm::Pnpm) {
        let cwd = std::env::current_dir()?;
        let root_path = init_project_root(&cwd).await?;
        migrate_from_pnpm(&root_path).await?;
    }
    Ok(())
}

pub async fn update_packages(
    action: PackageAction,
    specs: &[&str],
    workspace: Option<String>,
    scripts: ScriptPolicy,
    save_type: SaveType,
) -> Result<()> {
    let omit = get_omit();
    InstallService::update_packages(action, specs, workspace, scripts, save_type, &omit).await
}

pub async fn install(scripts: ScriptPolicy, root_path: &Path) -> Result<()> {
    let omit = get_omit();
    InstallService::install(scripts, root_path, &omit).await
}

pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
    // Parameter validation
    if npm_spec.trim().is_empty() {
        anyhow::bail!("Package specification cannot be empty");
    }

    // Resolve the effective prefix: CLI flag > UTOO_PREFIX env > config.
    let prefix = resolve_global_prefix(prefix).await;

    // Dispatch to service
    InstallService::install_global_package(npm_spec, prefix.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_install_global_package_empty_spec() {
        // Test installing with empty package spec
        let result = install_global_package("", None).await;
        assert!(result.is_err(), "Should fail with empty package spec");

        let result = install_global_package("   ", None).await;
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

    #[tokio::test]
    async fn test_uninstall_empty_specs() {
        let result = uninstall(Vec::new(), None, ScriptPolicy::Run).await;
        assert!(result.is_err(), "Should fail with empty specs");
    }
}
