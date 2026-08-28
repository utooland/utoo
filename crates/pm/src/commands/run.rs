//! Package-script execution command.

use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Stdio;
use std::time::Instant;

use crate::error::{CliError, ErrorKind};
use crate::helper::fuzzy_select;
use crate::helper::workspace::update_cwd_to_project;
use crate::model::cli_output::{
    CapturedOutput, CustomResult, ErrorDetails, ExecutionStatus, LifecycleExecution, PartialResult,
    ProcessExecution, RunPartialResult, RunResult, SkippedExecution,
};
use crate::model::package::PackageInfo;
use crate::service::config::ConfigService;
use crate::service::script::{MachineLifecycleOutcome, MissingScript, ScriptService};
use crate::service::workspace::{ResolvedWorkspaces, WorkspaceFilter, WorkspaceService};
use crate::util::cli_enum::ConfigScope;
use crate::util::config_file::Config;
use crate::util::invocation;
use crate::util::presenter::emit;

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
        if !invocation::interactive() {
            return Err(
                CliError::usage("a script name is required in non-interactive mode")
                    .with_suggestion("run `utoo run <script>`")
                    .into(),
            );
        }
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

    if invocation::json() {
        return run_machine(&updated_cwd, &script_name, resolved, missing, script_args).await;
    }

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

/// Fallback for `utoo <name>` when `<name>` is not a built-in subcommand:
/// prefer a custom command from the config, otherwise run `<name>` as a
/// package.json script.
pub async fn fallback(
    script_name: &str,
    filter: WorkspaceFilter,
    script_args: Vec<String>,
) -> Result<()> {
    // First check if there's a custom command configured for this script name
    let config = Config::load(ConfigScope::Local).await?;
    let config_service = ConfigService::new(config);
    if let Ok(Some(configured_command)) = config_service.get_available_cmd(script_name) {
        if invocation::json() {
            invocation::set_command("custom", None);
            return run_custom_machine(script_name, &configured_command, &script_args).await;
        }
        config_service.execute_command(script_name, &script_args)?;
        return Ok(());
    }

    // If no custom command found, try to run as script
    let script_args = if script_args.is_empty() {
        None
    } else {
        Some(script_args)
    };
    run(Some(script_name), filter, MissingScript::Fail, script_args).await
}

async fn run_machine(
    root: &std::path::Path,
    script: &str,
    resolved: ResolvedWorkspaces,
    missing: MissingScript,
    script_args: Option<Vec<String>>,
) -> Result<()> {
    let args = script_args.unwrap_or_default();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut completed = Vec::new();
    let mut skipped = Vec::new();

    match resolved {
        ResolvedWorkspaces::Current => {
            let package = PackageInfo::load(root).await?;
            let outcome =
                ScriptService::run_lifecycle_machine(&package, script, &args, None, missing).await;
            collect_machine_outcome(outcome, &package, None, &mut completed, &mut skipped)?;
        }
        ResolvedWorkspaces::Layers { layers, paths } => {
            for layer in layers {
                let mut names = layer;
                names.sort_unstable();
                let outcomes = futures::future::join_all(names.into_iter().filter_map(|name| {
                    let path = paths.get(&name)?.clone();
                    let args = args.clone();
                    Some(async move {
                        let package = PackageInfo::load(&path).await?;
                        let outcome = ScriptService::run_lifecycle_machine(
                            &package,
                            script,
                            &args,
                            Some(&name),
                            missing,
                        )
                        .await;
                        Ok::<_, anyhow::Error>((name, package, outcome))
                    })
                }))
                .await;

                let mut failures = Vec::new();
                let mut failure_messages = Vec::new();
                for outcome in outcomes {
                    let (name, package, outcome) = outcome?;
                    for execution in &outcome.executions {
                        if matches!(execution.status, ExecutionStatus::Succeeded) {
                            completed.push(execution.clone());
                        } else {
                            failures.push(execution.clone());
                        }
                    }
                    if outcome.skipped {
                        skipped.push(SkippedExecution {
                            package: (!package.name.is_empty()).then(|| package.name.clone()),
                            workspace: Some(name.clone()),
                            cwd: package.path.to_string_lossy().into_owned(),
                        });
                    }
                    if let Some(failure) = outcome.failure {
                        failure_messages.push(format!("{name}: {failure}"));
                    }
                }
                if !failure_messages.is_empty() {
                    return Err(run_failure(
                        failure_messages.join("; "),
                        completed,
                        failures,
                    ));
                }
            }
        }
    }

    emit(
        "run",
        &RunResult {
            script: script.to_string(),
            executions: completed,
            skipped,
        },
        || Ok(()),
    )
}

fn collect_machine_outcome(
    outcome: MachineLifecycleOutcome,
    package: &PackageInfo,
    workspace: Option<&str>,
    completed: &mut Vec<LifecycleExecution>,
    skipped: &mut Vec<SkippedExecution>,
) -> Result<()> {
    let mut failures = Vec::new();
    for execution in outcome.executions {
        if matches!(execution.status, ExecutionStatus::Succeeded) {
            completed.push(execution);
        } else {
            failures.push(execution);
        }
    }
    if outcome.skipped {
        skipped.push(SkippedExecution {
            package: (!package.name.is_empty()).then(|| package.name.clone()),
            workspace: workspace.map(str::to_string),
            cwd: package.path.to_string_lossy().into_owned(),
        });
    }
    if let Some(failure) = outcome.failure {
        return Err(run_failure(failure, completed.clone(), failures));
    }
    Ok(())
}

fn run_failure(
    message: String,
    completed: Vec<LifecycleExecution>,
    failures: Vec<LifecycleExecution>,
) -> anyhow::Error {
    let (kind, code) = if failures.is_empty() {
        (ErrorKind::NotFound, "script_not_found")
    } else {
        (ErrorKind::Local, "script_failed")
    };
    let mut error = CliError::new(kind, message).with_code(code);
    if !completed.is_empty() {
        error = error.with_partial_result(PartialResult::Run(RunPartialResult {
            executions: completed,
        }));
    }
    if !failures.is_empty() {
        error = error.with_details(ErrorDetails::Lifecycle {
            executions: failures,
        });
    }
    error.into()
}

async fn run_custom_machine(name: &str, configured: &str, args: &[String]) -> Result<()> {
    let mut parts = configured.split_whitespace();
    let Some(program) = parts.next() else {
        return Err(CliError::usage(format!(
            "invalid command alias for '{name}': '{configured}'"
        ))
        .into());
    };
    let cwd = std::env::current_dir()?;
    let mut command = std::process::Command::new(program);
    command
        .args(parts)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let full_command = std::iter::once(configured.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");
    let started = Instant::now();
    let output = tokio::process::Command::from(command).output().await;
    let execution = match output {
        Ok(output) => ProcessExecution {
            command: full_command,
            cwd: cwd.to_string_lossy().into_owned(),
            status: if output.status.success() {
                ExecutionStatus::Succeeded
            } else {
                ExecutionStatus::Failed
            },
            exit_code: Some(process_exit_code(&output.status) as u32),
            stdout: CapturedOutput::from_bytes(&output.stdout),
            stderr: CapturedOutput::from_bytes(&output.stderr),
            duration_ms: started.elapsed().as_millis() as u64,
        },
        Err(error) => {
            let execution = ProcessExecution {
                command: full_command,
                cwd: cwd.to_string_lossy().into_owned(),
                status: ExecutionStatus::FailedToStart,
                exit_code: None,
                stdout: CapturedOutput::empty(),
                stderr: CapturedOutput::empty(),
                duration_ms: started.elapsed().as_millis() as u64,
            };
            return Err(CliError::new(
                ErrorKind::Local,
                format!("failed to start custom command '{name}': {error}"),
            )
            .with_code("process_start_failed")
            .with_details(ErrorDetails::Process { execution })
            .into());
        }
    };
    if !matches!(execution.status, ExecutionStatus::Succeeded) {
        return Err(CliError::new(
            ErrorKind::Local,
            format!(
                "custom command '{name}' failed with exit code {}",
                execution.exit_code.unwrap_or(1)
            ),
        )
        .with_code("process_failed")
        .with_details(ErrorDetails::Process { execution })
        .into());
    }
    emit(
        "custom",
        &CustomResult {
            name: name.to_string(),
            configured_command: configured.to_string(),
            execution,
        },
        || Ok(()),
    )
}

fn process_exit_code(status: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
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
