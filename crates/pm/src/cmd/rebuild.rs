use crate::helper::lock::ensure_package_lock;
use crate::service::rebuild::RebuildService;
use crate::service::script::ScriptOutput;
use crate::util::cli_enum::ScriptPolicy;
use crate::util::logger::{finish_progress_bar, start_progress_bar};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

pub async fn rebuild(root_path: &Path) -> Result<()> {
    start_progress_bar();
    let resolve_start = Instant::now();
    let package_lock = ensure_package_lock(root_path).await?;
    finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));

    RebuildService::rebuild(
        &package_lock,
        root_path,
        ScriptPolicy::Run,
        ScriptOutput::Verbose,
    )
    .await
}
