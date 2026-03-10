use crate::service::update::clean_package_lock;
use crate::{
    cmd::install::install, helper::workspace::update_cwd_to_root, util::config_file::Config,
};
use anyhow::{Context, Result};

pub async fn update(ignore_scripts: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let root_path = update_cwd_to_root(&cwd).await?;
    Config::init_local(&root_path);

    // Clean package-lock.json
    tracing::debug!("Cleaning package-lock.json...");
    clean_package_lock(&root_path)
        .await
        .context("Failed to clean package-lock.json")?;

    // // Clean node_modules
    // tracing::debug!("Cleaning node_modules...");
    // clean_node_modules().await?;

    // Install dependencies
    install(ignore_scripts, &root_path).await?;

    Ok(())
}
