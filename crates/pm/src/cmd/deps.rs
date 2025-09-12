use anyhow::Result;
use std::path::Path;

use crate::service::dependency_resolution::DependencyResolutionService;

pub async fn build_deps(cwd: &Path) -> Result<()> {
    // Dispatch to service
    DependencyResolutionService::build_deps(cwd).await
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    // Dispatch to service
    DependencyResolutionService::build_workspace(cwd).await
}
