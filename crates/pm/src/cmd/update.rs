use crate::service::update::clean_package_lock;
use crate::util::cli_enum::{ReifyMode, ScriptPolicy};
use crate::{cmd::install::install_with_mode, helper::workspace::init_project_root};
use anyhow::{Context, Result};
use clap::Args;

#[derive(Args)]
pub struct UpdateArgs {
    /// Force reinstallation of all resolved packages
    #[arg(short, long)]
    pub force: bool,
}

pub async fn update(args: UpdateArgs, scripts: ScriptPolicy) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let root_path = init_project_root(&cwd).await?;

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

    Ok(())
}
