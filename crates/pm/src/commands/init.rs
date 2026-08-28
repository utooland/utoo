//! Package initialization command.

use std::path::PathBuf;

pub use crate::service::init::InitOutput;

/// Initialize a package in `project_dir`, or in the current directory when it
/// is omitted.
pub async fn run(
    mode: crate::types::InitMode,
    output: InitOutput,
    project_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    crate::service::init::init(mode, output, project_dir.as_deref()).await
}
