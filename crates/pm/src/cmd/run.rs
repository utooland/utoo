use std::path::Path;

use crate::helper::fuzzy_select;
use crate::helper::package::parse_package_name;
use crate::helper::workspace::{find_workspace_path, update_cwd_to_project};
use crate::model::package::{PackageInfo, Scripts};
use crate::service::script::ScriptService;
use crate::service::workspace::WorkspaceService;
use crate::util::json::load_package_json_from_path;
use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::Value;
use tokio::task::JoinSet;

/// Unified run function that handles both single workspace and multi-workspace script execution
pub async fn run(
    script_name: Option<&str>,
    workspace: Option<&str>,
    workspaces: bool,
    script_args: Option<Vec<String>>,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let updated_cwd = update_cwd_to_project(&cwd).await?;

    // If no script name provided, use fuzzy select
    let (script_name, selected_workspace) = if let Some(name) = script_name {
        (name.to_string(), workspace.map(|w| w.to_string()))
    } else {
        // Use fuzzy select (workspace-aware if applicable)
        let selection = fuzzy_select::select_script(&updated_cwd, workspace).await?;

        let equivalent_cmd = if let Some(ws) = &selection.workspace_name {
            format!("utoo run {} --workspace {}", selection.script_name, ws)
        } else {
            format!("utoo run {}", selection.script_name)
        };
        println!("{} {}", ">".bright_cyan(), equivalent_cmd.bright_black());
        println!();

        (selection.script_name, selection.workspace_name)
    };

    match workspaces {
        true => {
            // Run script in all workspaces with topological ordering
            run_script_in_all_workspaces(&script_name, script_args).await
        }
        false => {
            // Run script in specific workspace or current workspace
            let script_args_refs = script_args
                .as_ref()
                .map(|args| args.iter().map(|s| s.as_str()).collect::<Vec<&str>>());
            run_script(
                &script_name,
                selected_workspace.as_deref(),
                script_args_refs,
            )
            .await
        }
    }
}

pub async fn run_script(
    script_name: &str,
    workspace: Option<&str>,
    script_args: Option<Vec<&str>>,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let updated_cwd = update_cwd_to_project(&cwd).await?;
    let pkg = if let Some(workspace_name) = &workspace {
        let workspace_dir = find_workspace_path(
            &std::env::current_dir().context("Failed to get current directory")?,
            workspace_name,
        )
        .await
        .context("Failed to find workspace path")?;
        tracing::debug!(
            "Using workspace: {} at path: {}",
            workspace_name,
            workspace_dir.display()
        );
        load_package_json_from_path(&workspace_dir).await?
    } else {
        load_package_json_from_path(&updated_cwd).await?
    };

    let (scope, name, fullname) =
        parse_package_name(pkg.get("name").and_then(|v| v.as_str()).unwrap_or_default());

    let package = PackageInfo {
        path: if let Some(workspace_name) = workspace {
            find_workspace_path(
                &std::env::current_dir().context("Failed to get current directory")?,
                workspace_name,
            )
            .await
            .context("Failed to find workspace path")?
        } else {
            std::env::current_dir().context("Failed to get current directory")?
        },
        bin_files: Default::default(),
        scripts: Scripts::default(),
        scope,
        fullname,
        name,
        version: pkg
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    };

    // Get all scripts from package.json
    let scripts = if let Some(Value::Object(scripts)) = pkg.get("scripts") {
        scripts
    } else {
        anyhow::bail!("No scripts found in package.json");
    };

    // Execute pre script if exists
    let pre_script_name = format!("pre{script_name}");
    if let Some(Value::String(pre_script)) = scripts.get(&pre_script_name) {
        tracing::debug!(cmd = %pre_script, args = "", "Executing command");
        println!("> {pre_script} ");
        println!();
        ScriptService::execute_custom_script(&package, &pre_script_name, pre_script)
            .await
            .with_context(|| format!("Failed to execute pre script {pre_script_name}"))?;
    }

    // Execute main script
    let script_content = if let Some(Value::String(content)) = scripts.get(script_name) {
        content
    } else {
        anyhow::bail!("Script '{script_name}' not found in package.json");
    };

    let script_args = script_args.unwrap_or_default();
    let script_args_str = script_args.join(" ");
    tracing::debug!(cmd = %script_content, args = %script_args_str, "Executing command");
    println!("> {script_content} {script_args_str}");
    println!();
    ScriptService::execute_custom_script_with_args(
        &package,
        script_name,
        script_content,
        script_args,
    )
    .await
    .with_context(|| format!("Failed to execute script {script_name}"))?;

    // Execute post script if exists
    let post_script_name = format!("post{script_name}");
    if let Some(Value::String(post_script)) = scripts.get(&post_script_name) {
        tracing::debug!(cmd = %post_script, args = "", "Executing command");
        println!("> {post_script} ");
        println!();
        ScriptService::execute_custom_script(&package, &post_script_name, post_script)
            .await
            .with_context(|| format!("Failed to execute post script {post_script_name}"))?;
    }

    Ok(())
}

/// Check if a workspace has the specified script configured
async fn need_run(cwd: &Path, workspace_name: &str, script_name: &str) -> Result<bool> {
    // Find the workspace path
    let workspace_dir = match find_workspace_path(cwd, workspace_name).await {
        Ok(path) => path,
        Err(_) => {
            tracing::debug!("Workspace '{workspace_name}' not found, skipping");
            return Ok(false);
        }
    };

    // Load package.json from workspace
    let pkg = match load_package_json_from_path(&workspace_dir).await {
        Ok(pkg) => pkg,
        Err(_) => {
            tracing::debug!("No package.json found in workspace '{workspace_name}', skipping");
            return Ok(false);
        }
    };

    // Check if the script exists in package.json
    if let Some(Value::Object(scripts)) = pkg.get("scripts") {
        let has_script = scripts.contains_key(script_name);
        if !has_script {
            tracing::debug!(
                "Script '{script_name}' not found in workspace '{workspace_name}', skipping"
            );
        }
        Ok(has_script)
    } else {
        tracing::debug!("No scripts section found in workspace '{workspace_name}', skipping");
        Ok(false)
    }
}

/// Run script in all workspaces with topological ordering and concurrent execution
pub async fn run_script_in_all_workspaces(
    script_name: &str,
    script_args: Option<Vec<String>>,
) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let updated_cwd = update_cwd_to_project(&cwd).await?;

    tracing::debug!("Getting workspace topology for script: {script_name}");

    // Get topological ordering of workspaces
    let topology = WorkspaceService::get_workspace_topology(&updated_cwd).await?;

    if topology.is_empty() {
        tracing::debug!("No workspaces found or no valid topology");
        return Ok(());
    }

    tracing::debug!("Found {} topological layers", topology.len());

    // Execute scripts layer by layer (dependencies first)
    for (layer_index, layer) in topology.iter().enumerate() {
        if layer.is_empty() {
            continue;
        }

        // Filter workspaces that actually have the script configured
        let mut workspaces_to_run = Vec::new();
        for workspace_name in layer {
            if need_run(&cwd, workspace_name, script_name).await? {
                workspaces_to_run.push(workspace_name.clone());
            }
        }

        if workspaces_to_run.is_empty() {
            tracing::debug!(
                "Layer {}: No workspaces have script '{}', skipping layer",
                layer_index + 1,
                script_name
            );
            continue;
        }

        tracing::debug!(
            "Executing layer {}: {} workspaces (out of {} total)",
            layer_index + 1,
            workspaces_to_run.len(),
            layer.len()
        );

        // Create a JoinSet for concurrent execution within the same layer
        let mut join_set = JoinSet::new();

        for workspace_name in workspaces_to_run {
            let script_name = script_name.to_string();
            let script_args = script_args.clone();

            // Spawn concurrent task for each workspace in the layer
            join_set.spawn(async move {
                tracing::debug!(
                    "Running script '{script_name}' in workspace '{workspace_name}'"
                );

                let script_args_refs = script_args
                    .as_ref()
                    .map(|args| args.iter().map(|s| s.as_str()).collect::<Vec<&str>>());
                match run_script(&script_name, Some(&workspace_name), script_args_refs).await {
                    Ok(()) => {
                        tracing::debug!(
                            "Successfully completed script '{script_name}' in workspace '{workspace_name}'"
                        );
                        Ok(workspace_name)
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Failed to run script '{script_name}' in workspace '{workspace_name}': {e}"
                        );
                        Err((workspace_name, e))
                    }
                }
            });
        }

        // Wait for all tasks in this layer to complete
        let mut results = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(task_result) => results.push(task_result),
                Err(join_error) => {
                    return Err(anyhow::anyhow!("Task join error: {join_error}"));
                }
            }
        }

        // Check if any workspace failed in this layer
        let mut failed_workspaces = Vec::new();
        for result in results {
            match result {
                Ok(_workspace_name) => {
                    // Success, continue
                }
                Err((workspace_name, error)) => {
                    failed_workspaces.push((workspace_name, error));
                }
            }
        }

        // If any workspace failed, stop execution
        if !failed_workspaces.is_empty() {
            let error_messages: Vec<String> = failed_workspaces
                .iter()
                .map(|(name, err)| format!("{name}: {err}"))
                .collect();

            return Err(anyhow::anyhow!(
                "Script execution failed in layer {}: {}",
                layer_index + 1,
                error_messages.join(", ")
            ));
        }

        tracing::debug!("Layer {} completed successfully", layer_index + 1);
    }

    tracing::debug!("Successfully completed script '{script_name}' in all workspaces");
    Ok(())
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_run_script_not_found() {
        let _dir = tempdir().unwrap();
        let package_json = r#"
        {
            "name": "@test/package",
            "version": "1.0.0",
            "scripts": {
                "test": "exit 0"
            }
        }"#;

        fs::write(_dir.path().join("package.json"), package_json).unwrap();
        std::env::set_current_dir(_dir.path()).unwrap();

        let result = run_script("nonexistent", None, None).await;

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
        let _dir = tempdir().unwrap();
        let invalid_json = r#"{ "name": "test", "scripts": { "test": 123 } }"#;

        fs::write(_dir.path().join("package.json"), invalid_json).unwrap();
        std::env::set_current_dir(_dir.path()).unwrap();

        let result = run_script("test", None, None).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_topo_without_workspaces() {
        let _dir = tempdir().unwrap();
        let package_json = r#"
        {
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "test": "echo test"
            }
        }"#;

        fs::write(_dir.path().join("package.json"), package_json).unwrap();
        std::env::set_current_dir(_dir.path()).unwrap();

        let result = WorkspaceService::get_workspace_topology(_dir.path()).await;
        assert!(result.is_ok());
        let topology = result.unwrap();
        // Should return empty topology for non-workspace projects
        assert!(topology.is_empty());
    }

    #[tokio::test]
    #[allow(unused_variables)]
    async fn test_need_run_with_script() {
        let _dir = tempdir().unwrap();

        // Create a workspace directory
        let workspace_dir = _dir.path().join("packages").join("pkg1");
        fs::create_dir_all(&workspace_dir).unwrap();

        // Create package.json with scripts
        let package_json = r#"
        {
            "name": "@test/pkg1",
            "version": "1.0.0",
            "scripts": {
                "build": "echo building",
                "test": "echo testing"
            }
        }"#;
        fs::write(workspace_dir.join("package.json"), package_json).unwrap();

        // Create root package.json with workspaces
        let root_package_json = r#"
        {
            "name": "root",
            "version": "1.0.0",
            "workspaces": ["packages/*"]
        }"#;
        fs::write(_dir.path().join("package.json"), root_package_json).unwrap();

        let cwd = _dir.path();

        // Test existing script
        let result = need_run(cwd, "@test/pkg1", "build").await;
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Test non-existing script
        let result = need_run(cwd, "@test/pkg1", "nonexistent").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());

        // Test non-existing workspace
        let result = need_run(cwd, "nonexistent-workspace", "build").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
