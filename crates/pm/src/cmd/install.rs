use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::helper::migrate::{FromPm, migrate_from_pnpm};
use crate::helper::workspace::init_project_root;
use crate::service::install::InstallService;
use crate::util::cli_enum::{OmitType, PackageAction, SaveType};
use crate::util::format_print::pluralized_package_count;
use crate::util::logger::log_time_end;
use crate::util::script_policy::ScriptPolicyArgs;
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

    /// Allow these packages to run install-time scripts (comma-separated,
    /// repeatable). `name` or `name@version`. A one-off, non-persisted allow for
    /// otherwise-unreviewed packages; it does NOT override an explicit deny.
    #[arg(long, value_delimiter = ',')]
    pub allow_scripts: Vec<String>,

    /// Fail the install if any package with an install script is unreviewed.
    #[arg(long)]
    pub strict_allow_scripts: bool,

    /// Run ALL dependency install scripts, bypassing the allowScripts policy.
    /// Migration escape hatch — prints a warning.
    #[arg(long)]
    pub dangerously_allow_all_scripts: bool,

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

    let script_args = ScriptPolicyArgs {
        ignore_scripts: args.ignore_scripts,
        dangerously_allow_all: args.dangerously_allow_all_scripts,
        strict: args.strict_allow_scripts,
        allow: args.allow_scripts,
    };

    if args.specs.is_empty() {
        install_cwd(&script_args).await
    } else if args.global {
        // For global installs, process packages one by one
        for spec in args.specs.iter() {
            install_global_package(spec, args.prefix.as_deref(), &script_args).await?;
        }
        log_time_end(&pluralized_package_count(args.specs.len(), "installed"));
        Ok(())
    } else {
        let save_type = SaveType::from_flags(args.save_dev, args.save_peer, args.save_optional);
        let spec_refs: Vec<&str> = args.specs.iter().map(|s| s.as_str()).collect();
        update_packages(
            PackageAction::Add,
            &spec_refs,
            args.workspace,
            &script_args,
            save_type,
        )
        .await?;
        // Log install result with correct singular/plural form in one line
        log_time_end(&pluralized_package_count(args.specs.len(), "installed"));
        Ok(())
    }
}

/// Entry point for the `uninstall` command.
pub async fn uninstall(
    specs: Vec<String>,
    workspace: Option<String>,
    args: &ScriptPolicyArgs,
) -> Result<()> {
    if specs.is_empty() {
        anyhow::bail!("Package specification is required for uninstall");
    }

    let spec_refs: Vec<&str> = specs.iter().map(|s| s.as_str()).collect();
    update_packages(
        PackageAction::Remove,
        &spec_refs,
        workspace,
        args,
        SaveType::Prod,
    )
    .await?;
    log_time_end(&pluralized_package_count(specs.len(), "uninstalled"));
    Ok(())
}

/// Install all dependencies of the project containing the current directory.
/// Shared by bare `utoo` and `utoo install` without specs.
pub async fn install_cwd(args: &ScriptPolicyArgs) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root_path = init_project_root(&cwd).await?;
    install(args, &root_path).await?;
    log_time_end("All packages installed");
    Ok(())
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
    args: &ScriptPolicyArgs,
    save_type: SaveType,
) -> Result<()> {
    let omit = get_omit();
    InstallService::update_packages(action, specs, workspace, args, save_type, &omit).await
}

pub async fn install(args: &ScriptPolicyArgs, root_path: &Path) -> Result<()> {
    let omit = get_omit();
    InstallService::install(args, root_path, &omit).await
}

pub async fn install_global_package(
    npm_spec: &str,
    prefix: Option<&str>,
    args: &ScriptPolicyArgs,
) -> Result<()> {
    // Parameter validation
    if npm_spec.trim().is_empty() {
        anyhow::bail!("Package specification cannot be empty");
    }

    // Resolve the effective prefix: CLI flag > UTOO_PREFIX env > config.
    let prefix = resolve_global_prefix(prefix).await;

    // Dispatch to service
    InstallService::install_global_package(npm_spec, prefix.as_deref(), args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_install_global_package_empty_spec() {
        // Test installing with empty package spec
        let result = install_global_package("", None, &ScriptPolicyArgs::default()).await;
        assert!(result.is_err(), "Should fail with empty package spec");

        let result = install_global_package("   ", None, &ScriptPolicyArgs::default()).await;
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
            &ScriptPolicyArgs::default(),
            SaveType::Prod,
        )
        .await;
        assert!(result.is_err(), "Should fail with empty specs");
    }

    #[tokio::test]
    async fn test_uninstall_empty_specs() {
        let result = uninstall(Vec::new(), None, &ScriptPolicyArgs::default()).await;
        assert!(result.is_err(), "Should fail with empty specs");
    }
}
