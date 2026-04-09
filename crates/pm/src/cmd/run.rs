use crate::helper::fuzzy_select;
use crate::helper::workspace::update_cwd_to_project;
use crate::service::script::{MissingScript, ScriptService};
use crate::service::workspace::{ResolvedWorkspaces, WorkspaceFilter, WorkspaceService};
use anyhow::{Context, Result};
use colored::Colorize;

pub async fn run(
    script_name: Option<&str>,
    filter: WorkspaceFilter,
    missing: MissingScript,
    script_args: Option<Vec<String>>,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let updated_cwd = update_cwd_to_project(&cwd).await?;

    let (script_name, filter) = if let Some(name) = script_name {
        (name.to_string(), filter)
    } else {
        let single_ws = match &filter {
            WorkspaceFilter::Selected(ws) if ws.len() == 1 => Some(ws[0].as_str()),
            _ => None,
        };
        let selection = fuzzy_select::select_script(&updated_cwd, single_ws).await?;

        let equivalent_cmd = if let Some(ws) = &selection.workspace_name {
            format!("utoo run {} --workspace {}", selection.script_name, ws)
        } else {
            format!("utoo run {}", selection.script_name)
        };
        println!("{} {}", ">".bright_cyan(), equivalent_cmd.bright_black());
        println!();

        let filter = match selection.workspace_name {
            Some(ws) => WorkspaceFilter::Selected(vec![ws]),
            None => WorkspaceFilter::Current,
        };
        (selection.script_name, filter)
    };

    let resolved = WorkspaceService::resolve_layers(&updated_cwd, filter).await?;

    match resolved {
        ResolvedWorkspaces::Current => {
            let script_args_refs = script_args
                .as_ref()
                .map(|args| args.iter().map(|s| s.as_str()).collect::<Vec<&str>>());
            ScriptService::run_script(&updated_cwd, &script_name, None, script_args_refs).await
        }
        ResolvedWorkspaces::Layers { layers, paths } => {
            ScriptService::run_in_layers(&layers, &paths, &script_name, missing, script_args).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_run_script_not_found() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"@test/package","version":"1.0.0","scripts":{"test":"exit 0"}}"#,
        )
        .unwrap();

        let result = ScriptService::run_script(dir.path(), "nonexistent", None, None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Script 'nonexistent' not found")
        );
    }

    #[tokio::test]
    async fn test_run_script_invalid_json() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "test", "scripts": { "test": 123 } }"#,
        )
        .unwrap();

        let result = ScriptService::run_script(dir.path(), "test", None, None).await;
        assert!(result.is_err());
    }
}
