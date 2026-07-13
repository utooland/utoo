use anyhow::{Context, Result};

use crate::helper::ruborist_context::Context as FsContext;
use crate::service::outdated::find_outdated;
use crate::service::workspace::WorkspaceFilter;
use crate::util::format_print::print_outdated;

pub async fn outdated(patterns: Vec<String>, workspace_filter: WorkspaceFilter) -> Result<bool> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let discovery = FsContext::discovery();
    let current_project = discovery.find_project_path(&cwd).await?;
    let root_path = discovery.find_root_path(&cwd).await?;
    let items = find_outdated(&root_path, &current_project, workspace_filter, &patterns).await?;
    print_outdated(&items);
    Ok(!items.is_empty())
}
