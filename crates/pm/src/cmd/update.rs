use crate::service::update::clean_package_lock;
use crate::util::logger::log_verbose;
use crate::{cmd::install::install, helper::workspace::update_cwd_to_root};
use anyhow::{Context, Result};

pub async fn update(ignore_scripts: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    // Clean all node_modules
    // Clean package-lock.json
    log_verbose("Cleaning package-lock.json...");
    clean_package_lock()
        .await
        .context("Failed to clean package-lock.json")?;
    let root_path = update_cwd_to_root(&cwd).await?;

    // // Clean node_modules
    // log_verbose("Cleaning node_modules...");
    // clean_node_modules().await?;

    // Install dependencies
    install(ignore_scripts, &root_path).await?;

    Ok(())
}
