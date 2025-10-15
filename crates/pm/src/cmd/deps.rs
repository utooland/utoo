use anyhow::Result;
use std::path::Path;

use crate::helper::lock::{PackageLock, save_package_lock};
use crate::service::dependency_resolution::DependencyResolutionService;

pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
    // 1. Build dependency tree
    let package_lock = DependencyResolutionService::build_deps(cwd).await?;

    // 2. Save to disk synchronously (ensure file is written before command exits)
    save_package_lock(cwd, &package_lock).await?;

    Ok(package_lock)
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    // Dispatch to service
    DependencyResolutionService::build_workspace(cwd).await
}
