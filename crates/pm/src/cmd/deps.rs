use anyhow::Result;
use std::path::Path;

use crate::service::dependency_resolution::DependencyResolutionService;

pub async fn build_deps(cwd: &Path) -> Result<()> {
    // Parameter validation
    if !cwd.exists() {
        return Err(anyhow::anyhow!("Working directory does not exist: {}", cwd.display()));
    }

    // Dispatch to service
    DependencyResolutionService::build_deps(cwd).await
}

pub async fn build_workspace(cwd: &Path) -> Result<()> {
    // Parameter validation
    if !cwd.exists() {
        return Err(anyhow::anyhow!("Working directory does not exist: {}", cwd.display()));
    }

    // Dispatch to service
    DependencyResolutionService::build_workspace(cwd).await
}
