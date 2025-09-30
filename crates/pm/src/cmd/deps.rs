use anyhow::Result;
use std::path::Path;

use crate::helper::lock::PackageLock;
use crate::service::dependency_resolution::{DependencyResolutionService, set_concurrency};

pub async fn build_deps(cwd: &Path, con: Option<usize>) -> Result<PackageLock> {
    // Set concurrency if provided
    if let Some(concurrency) = con {
        set_concurrency(concurrency);
    }
    DependencyResolutionService::preload(cwd).await?;
    let package_lock = DependencyResolutionService::build_deps(cwd).await?;
    Ok(package_lock)
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    // Dispatch to service
    DependencyResolutionService::build_workspace(cwd).await
}
