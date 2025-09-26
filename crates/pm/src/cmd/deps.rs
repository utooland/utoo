use anyhow::Result;
use std::path::Path;

use crate::helper::lock::PackageLock;
use crate::service::dependency_resolution::DependencyResolutionService;

pub async fn build_deps(cwd: &Path) -> Result<PackageLock> {
    // Dispatch to service - now returns PackageLock directly
    DependencyResolutionService::build_deps(cwd).await
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    // Dispatch to service
    DependencyResolutionService::build_workspace(cwd).await
}
