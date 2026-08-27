use crate::service::update::clean_package_lock;
use crate::util::cli_enum::{ReifyMode, ScriptPolicy};
use crate::util::install_progress::DownloadBaseline;
use crate::util::invocation;
use crate::util::presenter::emit;
use crate::{cmd::install::install_with_mode, helper::workspace::init_project_root};
use anyhow::{Context, Result};
use clap::Args;

use crate::model::cli_output::{
    DependencyOperation, DependencyScope, UpdateResult, UpdatedPackage,
};

#[derive(Args)]
pub struct UpdateArgs {
    /// Force reinstallation of all resolved packages
    #[arg(short, long)]
    pub force: bool,
}

pub async fn update(args: UpdateArgs, scripts: ScriptPolicy) -> Result<()> {
    let machine = invocation::json();
    let download_baseline = machine.then(DownloadBaseline::capture);
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let root_path = init_project_root(&cwd).await?;
    let before = if machine {
        super::install::load_lock_snapshot(Some(&root_path)).await
    } else {
        None
    };
    let before_direct = before
        .as_ref()
        .map(|lock| super::install::direct_packages(lock, &[]))
        .unwrap_or_default();

    // Clean package-lock.json
    tracing::debug!("Cleaning package-lock.json...");
    clean_package_lock(&root_path)
        .await
        .context("Failed to clean package-lock.json")?;

    // Install dependencies
    let mode = if args.force {
        ReifyMode::Force
    } else {
        ReifyMode::Incremental
    };
    install_with_mode(scripts, &root_path, mode).await?;

    if !machine {
        return Ok(());
    }
    let after = super::install::load_lock_snapshot(Some(&root_path)).await;
    let after_direct = after
        .as_ref()
        .map(|lock| super::install::direct_packages(lock, &[]))
        .unwrap_or_default();
    let mut updated = after_direct
        .iter()
        .filter_map(|package| {
            let before = before_direct
                .iter()
                .find(|candidate| candidate.name == package.name)?;
            (before.version != package.version).then(|| UpdatedPackage {
                name: package.name.clone(),
                from_version: before.version.clone(),
                to_version: package.version.clone(),
            })
        })
        .collect::<Vec<_>>();
    updated.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    let output = UpdateResult {
        operation: DependencyOperation::Update,
        scope: DependencyScope::Local,
        workspace: None,
        force: args.force,
        updated,
        summary: super::install::dependency_summary(
            before.as_ref(),
            after.as_ref(),
            download_baseline.map_or(0, |baseline| baseline.downloaded_bytes()),
        ),
    };
    emit("update", &output, || Ok(()))
}
