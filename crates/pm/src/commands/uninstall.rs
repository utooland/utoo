//! Dependency removal command.

use anyhow::Result;

use super::install::{dependency_summary, direct_packages, load_lock_snapshot, update_packages};
use crate::helper::workspace::init_project_root;
use crate::model::cli_output::{DependencyOperation, DependencyScope, UninstallResult};
use crate::util::cli_enum::{PackageAction, SaveType, ScriptPolicy};
use crate::util::format_print::pluralized_package_count;
use crate::util::invocation;
use crate::util::logger::log_time_end;
use crate::util::presenter::emit;

/// Remove dependencies from the current project.
pub async fn run(
    specs: Vec<String>,
    workspace: Option<String>,
    scripts: ScriptPolicy,
) -> Result<()> {
    if specs.is_empty() {
        anyhow::bail!("Package specification is required for uninstall");
    }

    let machine = invocation::json();
    let root_path = if machine {
        let cwd = std::env::current_dir()?;
        Some(init_project_root(&cwd).await?)
    } else {
        None
    };
    let before = load_lock_snapshot(root_path.as_deref()).await;
    let removed = before
        .as_ref()
        .map(|lock| direct_packages(lock, &specs))
        .unwrap_or_default();
    let spec_refs = specs.iter().map(String::as_str).collect::<Vec<_>>();
    update_packages(
        PackageAction::Remove,
        &spec_refs,
        workspace.clone(),
        scripts,
        SaveType::Prod,
    )
    .await?;
    log_time_end(&pluralized_package_count(specs.len(), "uninstalled"));
    if !machine {
        return Ok(());
    }
    let after = load_lock_snapshot(root_path.as_deref()).await;
    emit(
        "uninstall",
        &UninstallResult {
            operation: DependencyOperation::Remove,
            scope: DependencyScope::Local,
            workspace,
            requested: specs,
            removed,
            summary: dependency_summary(before.as_ref(), after.as_ref(), 0),
        },
        || Ok(()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_an_empty_package_list() {
        let result = run(Vec::new(), None, ScriptPolicy::Run).await;
        assert!(result.is_err());
    }
}
