use crate::service::package::PackageService;
use anyhow::{Context, Result};
use std::path::Path;

pub async fn rebuild(root_path: &Path) -> Result<()> {
    let packages = PackageService::collect_packages(root_path)
        .map_err(|e| anyhow::anyhow!("Failed to collect packages: {}", e))?;

    let execution_queues = PackageService::create_execution_queues(packages)
        .context("Failed to create execution queues")?;
    PackageService::execute_queues(execution_queues)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute queues: {}", e))?;

    // Handle project's own rebuild logic
    // Since package-lock.json doesn't have root node information for "", need to manually add
    // We align with npm cli's logic: after dependency reify completes, manually execute project's own scripts
    // const scripts = [
    //     'preinstall',
    //     'install',
    //     'postinstall',
    //     'prepublish', // XXX(npm9) should we remove this finally??
    //     'preprepare',
    //     'prepare',
    //     'postprepare',
    //   ]

    PackageService::process_project_hooks(root_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to process project hooks: {}", e))?;

    Ok(())
}
