use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::FuzzySelect;
use std::path::Path;

use crate::helper::ruborist_context::Context as FsContext;
use crate::util::user_config::get_or_load_package_json;
use utoo_ruborist::manifest::PackageJson;

/// Represents a script selection with its workspace context
#[derive(Debug, Clone)]
pub struct ScriptSelection {
    /// The script name
    pub script_name: String,
    /// The workspace name (None for non-workspace or root scripts)
    pub workspace_name: Option<String>,
}

/// Represents a script item with optional workspace context
#[derive(Debug, Clone)]
struct ScriptItem {
    workspace_name: Option<String>,
    script_name: String,
    command: String,
}

impl ScriptItem {
    /// Format the script item for display with colors
    fn format_display(&self) -> String {
        let truncated_cmd = if self.command.len() > 60 {
            format!("{}...", &self.command[..60])
        } else {
            self.command.clone()
        };

        match &self.workspace_name {
            Some(workspace) => {
                // workspace/script - command
                // workspace: cyan, script: green
                format!(
                    "{}/{} - {}",
                    workspace.cyan(),
                    self.script_name.green(),
                    truncated_cmd.bright_black(),
                )
            }
            None => {
                // script - command
                // script: green
                format!("{} - {}", self.script_name.green(), truncated_cmd)
            }
        }
    }
}

/// Collect scripts from a PackageJson
fn collect_scripts_from_package(
    pkg: &PackageJson,
    workspace_name: Option<String>,
) -> Vec<ScriptItem> {
    pkg.scripts_or_empty()
        .iter()
        .map(|(script_name, cmd)| ScriptItem {
            workspace_name: workspace_name.clone(),
            script_name: script_name.clone(),
            command: cmd.clone(),
        })
        .collect()
}

/// Select a script interactively from package.json
///
/// # Parameters
/// - `cwd`: Current working directory
/// - `workspace_filter`: Optional workspace name to filter scripts
///   - `None`: Show all scripts (workspace-aware if applicable, or single package)
///   - `Some(name)`: Show only scripts from the specified workspace
pub async fn select_script(cwd: &Path, workspace_filter: Option<&str>) -> Result<ScriptSelection> {
    let pkg = get_or_load_package_json(cwd).await?;

    // Check if this is a workspace root
    let is_workspace_root = pkg.workspaces.is_some();

    let script_items = if is_workspace_root && workspace_filter.is_none() {
        // Collect scripts from root package (should be first)
        let mut items = collect_scripts_from_package(&pkg, None);
        items.sort_by(|a, b| a.script_name.cmp(&b.script_name));

        // Collect scripts from all workspaces
        let workspaces = FsContext::discovery().find_workspaces(cwd).await?;
        let mut workspace_items: Vec<ScriptItem> = workspaces
            .into_iter()
            .flat_map(|ws| collect_scripts_from_package(&ws.package_json, Some(ws.name)))
            .collect();

        // Sort workspace items by workspace name, then script name
        workspace_items.sort_by(|a, b| {
            a.workspace_name
                .cmp(&b.workspace_name)
                .then_with(|| a.script_name.cmp(&b.script_name))
        });

        // Root scripts first, then workspace scripts
        items.extend(workspace_items);
        items
    } else if let Some(workspace_name) = workspace_filter {
        // Collect scripts from specific workspace
        let workspaces = FsContext::discovery().find_workspaces(cwd).await?;
        let ws = workspaces
            .into_iter()
            .find(|ws| ws.name == workspace_name)
            .ok_or_else(|| anyhow::anyhow!("Workspace '{}' not found", workspace_name))?;

        // Don't show workspace prefix when filtering by workspace
        let mut items = collect_scripts_from_package(&ws.package_json, None);
        items.sort_by(|a, b| a.script_name.cmp(&b.script_name));
        items
    } else {
        // Collect scripts from single package (non-workspace project)
        let mut items = collect_scripts_from_package(&pkg, None);
        items.sort_by(|a, b| a.script_name.cmp(&b.script_name));
        items
    };

    if script_items.is_empty() {
        anyhow::bail!("No scripts found");
    }

    // Format for display
    let display_items: Vec<String> = script_items
        .iter()
        .map(|item| item.format_display())
        .collect();

    let prompt = format!(
        "{} {}",
        ">".bright_cyan(),
        "Select a script to run".bright_cyan()
    );
    let selection = FuzzySelect::new()
        .with_prompt(&prompt)
        .items(&display_items)
        .default(0)
        .highlight_matches(false)
        .report(false) // Don't show selected item (we'll show equivalent command instead)
        .interact()
        .context("Failed to select script")?;

    let selected = &script_items[selection];
    Ok(ScriptSelection {
        script_name: selected.script_name.clone(),
        workspace_name: selected.workspace_name.clone(),
    })
}
