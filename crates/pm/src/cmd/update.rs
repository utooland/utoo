use crate::service::update::clean_package_lock;
use crate::util::script_policy::ScriptPolicyArgs;
use crate::{cmd::install::install, helper::workspace::init_project_root};
use anyhow::{Context, Result};

pub async fn update(args: &ScriptPolicyArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let root_path = init_project_root(&cwd).await?;

    // Clean package-lock.json
    tracing::debug!("Cleaning package-lock.json...");
    clean_package_lock(&root_path)
        .await
        .context("Failed to clean package-lock.json")?;

    // // Clean node_modules
    // tracing::debug!("Cleaning node_modules...");
    // clean_node_modules().await?;

    // Install dependencies
    install(args, &root_path).await?;

    Ok(())
}
