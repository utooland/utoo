//! Dependency lifecycle-script rebuild command.

use crate::helper::lock::ensure_package_lock;
use crate::model::cli_output::{DependencyOperation, RebuildResult, RebuildSummary};
use crate::service::rebuild::RebuildService;
use crate::service::script::ScriptOutput;
use crate::util::cli_enum::ScriptPolicy;
use crate::util::logger::{finish_progress_bar, start_progress_bar};
use crate::util::{invocation, presenter};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

pub async fn run(root_path: &Path) -> Result<()> {
    start_progress_bar();
    let resolve_start = Instant::now();
    let package_lock = ensure_package_lock(root_path).await?;
    finish_progress_bar("package-lock.json resolved", Some(resolve_start.elapsed()));

    let output = if invocation::json() {
        ScriptOutput::Machine
    } else {
        ScriptOutput::Verbose
    };
    RebuildService::rebuild(&package_lock, root_path, ScriptPolicy::Run, output).await?;

    if !invocation::json() {
        return Ok(());
    }
    let summary = RebuildSummary {
        packages: package_lock.packages.len().saturating_sub(1) as u64,
        scripts: package_lock
            .packages
            .values()
            .filter(|package| package.has_install_scripts())
            .count() as u64,
        bins: package_lock
            .packages
            .values()
            .map(|package| {
                package
                    .bin
                    .as_ref()
                    .map(|bin| bin.entries(package.name.as_deref().unwrap_or("")).len())
                    .unwrap_or_default()
            })
            .sum::<usize>() as u64,
    };
    presenter::emit(
        "rebuild",
        &RebuildResult {
            operation: DependencyOperation::Rebuild,
            summary,
        },
        || Ok(()),
    )
}
