use crate::helper::lock::ensure_package_lock;
use crate::service::rebuild::RebuildService;
use anyhow::Result;
use std::path::Path;

pub async fn rebuild(root_path: &Path) -> Result<()> {
    let build_result = ensure_package_lock(root_path).await?;

    // Wait for pipeline workers if any
    if let Some(handles) = build_result.pipeline_handles {
        handles.await_completion().await;
    }

    RebuildService::rebuild(&build_result.package_lock, root_path, false).await
}
