use crate::helper::lock::ensure_package_lock;
use crate::service::rebuild::RebuildService;
use crate::util::cli_enum::ScriptPolicy;
use anyhow::Result;
use std::path::Path;

pub async fn rebuild(root_path: &Path) -> Result<()> {
    let package_lock = ensure_package_lock(root_path).await?;
    RebuildService::rebuild(&package_lock, root_path, ScriptPolicy::Run).await
}
