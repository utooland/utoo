//! npm lifecycle orchestration: pre/post event chains, single-package runs,
//! and multi-workspace layered execution.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::task::JoinSet;

use super::ScriptService;
use super::exec::{ScriptFailure, status_exit_code};
use crate::helper::workspace::find_workspace_path;
use crate::model::cli_output::{CapturedOutput, ExecutionStatus, LifecycleExecution};
use crate::model::package::PackageInfo;
use crate::util::format_print::{
    announce_script, print_hook_done, print_layer_separator, print_multi_workspace_header,
    print_workspace_result,
};

/// How script output is handled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutput {
    /// Stream to terminal in real time (user-facing scripts).
    Verbose,
    /// Capture and only print on failure (dependency lifecycle scripts).
    Silent,
    /// Capture without writing to stdout/stderr (machine invocations).
    Machine,
}

/// What to do when a workspace doesn't have the requested script.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MissingScript {
    /// Fail with an error (default, matches `npm run --workspaces`).
    Fail,
    /// Skip silently (`--if-present`).
    Skip,
}

/// Where the output of a [`ScriptService::run_lifecycle`] call goes.
///
/// Capture borrows the caller's buffers so they retain bytes captured up to
/// a failing step — `run_in_layers` prints those as the `✗ [ws] …` tail.
pub enum LifecycleSink<'a> {
    Stream {
        workspace_label: Option<&'a str>,
        /// Emit a `✓ <event> [12.3s]` line on completion and a periodic
        /// `⏳ <event> still running [Ns]…` heartbeat while a step runs long.
        /// On for install lifecycle hooks (which are expected to finish); off
        /// for `utoo run`, where the script may be a long-lived dev server and
        /// the heartbeat would be noise.
        timed: bool,
    },
    Capture {
        workspace_label: &'a str,
        header: &'a mut String,
        body: &'a mut Vec<u8>,
    },
    Machine,
}

pub struct MachineLifecycleOutcome {
    pub executions: Vec<LifecycleExecution>,
    pub skipped: bool,
    pub failure: Option<String>,
}

impl ScriptService {
    pub async fn run_lifecycle_machine(
        package: &PackageInfo,
        event: &str,
        args: &[&str],
        workspace: Option<&str>,
        missing: MissingScript,
    ) -> MachineLifecycleOutcome {
        let pre_name = format!("pre{event}");
        let post_name = format!("post{event}");
        let main_script = package.scripts.get(event).cloned();
        if missing == MissingScript::Fail && main_script.is_none() {
            return MachineLifecycleOutcome {
                executions: Vec::new(),
                skipped: false,
                failure: Some(format!("Script '{event}' not found in package.json")),
            };
        }
        let steps: [(&str, Option<String>, &[&str]); 3] = [
            (
                pre_name.as_str(),
                package.scripts.get(&pre_name).cloned(),
                &[],
            ),
            (event, main_script, args),
            (
                post_name.as_str(),
                package.scripts.get(&post_name).cloned(),
                &[],
            ),
        ];
        let mut executions = Vec::new();
        for (step_name, script, step_args) in steps {
            let Some(script) = script else { continue };
            let command = if step_args.is_empty() {
                script.clone()
            } else {
                format!("{script} {}", step_args.join(" "))
            };
            let started = Instant::now();
            let captured = Self::execute_custom_script_captured(
                package,
                step_name,
                &script,
                step_args.to_vec(),
            )
            .await;
            let duration_ms = started.elapsed().as_millis() as u64;
            let execution = match captured {
                Ok(output) => LifecycleExecution {
                    package: (!package.name.is_empty()).then(|| package.name.clone()),
                    workspace: workspace.map(str::to_string),
                    event: step_name.to_string(),
                    command,
                    cwd: package.path.to_string_lossy().into_owned(),
                    status: if output.status.success() {
                        ExecutionStatus::Succeeded
                    } else {
                        ExecutionStatus::Failed
                    },
                    exit_code: Some(status_exit_code(&output.status) as u32),
                    stdout: CapturedOutput::from_bytes(&output.stdout),
                    stderr: CapturedOutput::from_bytes(&output.stderr),
                    duration_ms,
                },
                Err(error) => {
                    let execution = LifecycleExecution {
                        package: (!package.name.is_empty()).then(|| package.name.clone()),
                        workspace: workspace.map(str::to_string),
                        event: step_name.to_string(),
                        command,
                        cwd: package.path.to_string_lossy().into_owned(),
                        status: ExecutionStatus::FailedToStart,
                        exit_code: None,
                        stdout: CapturedOutput::empty(),
                        stderr: CapturedOutput::empty(),
                        duration_ms,
                    };
                    executions.push(execution);
                    return MachineLifecycleOutcome {
                        executions,
                        skipped: false,
                        failure: Some(format!("Failed to execute {step_name}: {error:#}")),
                    };
                }
            };
            let failed = !matches!(execution.status, ExecutionStatus::Succeeded);
            let exit_code = execution.exit_code;
            executions.push(execution);
            if failed {
                return MachineLifecycleOutcome {
                    executions,
                    skipped: false,
                    failure: Some(format!(
                        "Failed to execute {step_name}: exit code {}",
                        exit_code.unwrap_or(1)
                    )),
                };
            }
        }
        MachineLifecycleOutcome {
            skipped: executions.is_empty(),
            executions,
            failure: None,
        }
    }

    /// Run the npm event chain `pre<event>` / `<event>` / `post<event>` on a
    /// single package, mirroring npm's lifecycle semantics (e.g. `install`
    /// event = preinstall + install + postinstall, each independent).
    ///
    /// `missing` controls behaviour when the main script is undefined:
    /// `Fail` bails (matches `npm run <name>`), `Skip` runs whatever subset
    /// of pre/post exists. Pre/post are always silently skipped when absent.
    pub async fn run_lifecycle(
        package: &PackageInfo,
        event: &str,
        args: &[&str],
        mut sink: LifecycleSink<'_>,
        missing: MissingScript,
    ) -> Result<bool> {
        let pre_name = format!("pre{event}");
        let post_name = format!("post{event}");

        let main_script = package.scripts.get(event).cloned();
        let pre_script = package.scripts.get(&pre_name).cloned();
        let post_script = package.scripts.get(&post_name).cloned();

        if missing == MissingScript::Fail && main_script.is_none() {
            anyhow::bail!("Script '{event}' not found in package.json");
        }

        let steps: [(&str, Option<String>, &[&str]); 3] = [
            (pre_name.as_str(), pre_script, &[]),
            (event, main_script, args),
            (post_name.as_str(), post_script, &[]),
        ];

        let mut ran_any = false;
        for (step_name, script_opt, step_args) in steps {
            let Some(content) = script_opt else { continue };
            ran_any = true;

            match &mut sink {
                LifecycleSink::Stream {
                    workspace_label,
                    timed,
                } => {
                    announce_script(*workspace_label, &content, &step_args.join(" "));
                    let started = Instant::now();
                    // No "still running" heartbeat here: a streamed hook prints
                    // its own output live, so it already reads as working — the
                    // only addition a timed install hook needs is a uniform
                    // `✓ <hook> [Xs]` marker when it finishes. (The silent
                    // dependency-script path, which shows nothing, is where the
                    // heartbeat earns its keep — see `logger::ScriptHeartbeat`.)
                    Self::execute_custom_script(package, step_name, &content, step_args.to_vec())
                        .await
                        .with_context(|| format!("Failed to execute {step_name}"))?;
                    if *timed {
                        print_hook_done(*workspace_label, step_name, started.elapsed());
                    }
                }
                LifecycleSink::Capture {
                    workspace_label,
                    header,
                    body,
                } => {
                    writeln!(
                        header,
                        "[{}] {} {}",
                        workspace_label,
                        content,
                        step_args.join(" ")
                    )
                    .expect("writing a lifecycle header to String cannot fail");
                    let cap = Self::execute_custom_script_captured(
                        package,
                        step_name,
                        &content,
                        step_args.to_vec(),
                    )
                    .await?;
                    append_captured(body, &cap.stdout, &cap.stderr);
                    if !cap.status.success() {
                        anyhow::bail!("Failed to execute {step_name}");
                    }
                }
                LifecycleSink::Machine => {
                    let started = Instant::now();
                    let cap = Self::execute_custom_script_captured(
                        package,
                        step_name,
                        &content,
                        step_args.to_vec(),
                    )
                    .await
                    .map_err(|error| {
                        ScriptFailure::failed_to_start(
                            package,
                            step_name,
                            if step_args.is_empty() {
                                content.clone()
                            } else {
                                format!("{content} {}", step_args.join(" "))
                            },
                            started.elapsed(),
                            &error,
                        )
                    })?;
                    if !cap.status.success() {
                        return Err(ScriptFailure::lifecycle(
                            package,
                            step_name,
                            &content,
                            step_args,
                            &cap,
                            started.elapsed(),
                        )
                        .into());
                    }
                }
            }
        }

        Ok(ran_any)
    }

    /// Run a named script (with pre/post lifecycle) in a single package,
    /// streaming output to the terminal.
    pub async fn run_script(
        cwd: &Path,
        script_name: &str,
        workspace: Option<&str>,
        script_args: Option<Vec<&str>>,
    ) -> Result<()> {
        let package_path = if let Some(workspace_name) = workspace {
            let ws_dir = find_workspace_path(cwd, workspace_name)
                .await
                .context("Failed to find workspace path")?;
            tracing::debug!(
                "Using workspace: {} at {}",
                workspace_name,
                ws_dir.display()
            );
            ws_dir
        } else {
            cwd.to_path_buf()
        };

        let package = PackageInfo::load(&package_path).await?;
        let args = script_args.unwrap_or_default();

        Self::run_lifecycle(
            &package,
            script_name,
            &args,
            LifecycleSink::Stream {
                workspace_label: workspace,
                timed: false,
            },
            MissingScript::Fail,
        )
        .await?;

        Ok(())
    }

    /// Run a named script across multiple workspaces in topological layers.
    ///
    /// Same-layer workspaces execute concurrently; output is captured per
    /// workspace and printed grouped as each finishes (stream-as-complete).
    pub async fn run_in_layers(
        layers: &[Vec<String>],
        paths: &HashMap<String, PathBuf>,
        script_name: &str,
        missing: MissingScript,
        script_args: Option<Vec<String>>,
    ) -> Result<()> {
        if layers.is_empty() {
            return Ok(());
        }

        let layer_count = layers.len();
        print_multi_workspace_header(script_name, layers);

        for (layer_index, layer) in layers.iter().enumerate() {
            let failed_names = Self::run_layer(
                layer,
                paths,
                script_name,
                missing,
                script_args.as_deref(),
                layer_index,
                layer_count,
            )
            .await?;
            anyhow::ensure!(
                failed_names.is_empty(),
                "Script execution failed in layer {}: {}",
                layer_index + 1,
                failed_names.join(", ")
            );
        }

        Ok(())
    }

    /// Run one topological layer concurrently: spawn every workspace's
    /// captured script, print each result as it completes (separator before
    /// the first), and return the names that failed.
    async fn run_layer(
        layer: &[String],
        paths: &HashMap<String, PathBuf>,
        script_name: &str,
        missing: MissingScript,
        script_args: Option<&[String]>,
        layer_index: usize,
        layer_count: usize,
    ) -> Result<Vec<String>> {
        let workspaces_to_run: Vec<_> = layer
            .iter()
            .filter_map(|name| paths.get(name).map(|p| (name.clone(), p.clone())))
            .collect();

        if workspaces_to_run.is_empty() {
            return Ok(Vec::new());
        }

        let mut join_set = JoinSet::new();
        for (workspace_name, ws_path) in workspaces_to_run {
            let script_name = script_name.to_string();
            let script_args = script_args.map(<[String]>::to_vec);

            join_set.spawn(async move {
                run_script_captured(
                    &ws_path,
                    &script_name,
                    &workspace_name,
                    missing,
                    script_args,
                )
                .await
            });
        }

        let mut failed_names = Vec::new();
        let mut separator_printed = false;
        while let Some(outcome) = join_set.join_next().await.transpose()? {
            let WorkspaceOutcome::Ran {
                name,
                header,
                body,
                success,
            } = outcome
            else {
                continue;
            };
            if !separator_printed {
                print_layer_separator(layer_index, layer_count);
                separator_printed = true;
            }
            print_workspace_result(&header, &body, success);
            if !success {
                failed_names.push(name);
            }
        }
        Ok(failed_names)
    }
}

enum WorkspaceOutcome {
    Ran {
        name: String,
        header: String,
        body: Vec<u8>,
        success: bool,
    },
    Skipped,
}

async fn run_script_captured(
    workspace_dir: &Path,
    script_name: &str,
    workspace_name: &str,
    missing: MissingScript,
    script_args: Option<Vec<String>>,
) -> WorkspaceOutcome {
    let mut header = String::new();
    let mut body = Vec::new();

    let outcome = async {
        let package = PackageInfo::load(workspace_dir).await?;

        // Override run_lifecycle's generic "Script not found" error with a
        // CLI-friendly hint that names the workspace and points at `utoo run`.
        if missing == MissingScript::Fail && !package.scripts.contains_key(script_name) {
            anyhow::bail!(
                "Missing script: \"{script_name}\"\n\nTo see a list of scripts, run:\n  utoo run --workspace={workspace_name}"
            );
        }

        let args_refs: Vec<&str> = script_args
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|s| s.as_str())
            .collect();

        ScriptService::run_lifecycle(
            &package,
            script_name,
            &args_refs,
            LifecycleSink::Capture {
                workspace_label: workspace_name,
                header: &mut header,
                body: &mut body,
            },
            MissingScript::Skip,
        )
        .await
    }
    .await;

    match outcome {
        Ok(true) => WorkspaceOutcome::Ran {
            name: workspace_name.to_string(),
            header,
            body,
            success: true,
        },
        Ok(false) => WorkspaceOutcome::Skipped,
        Err(e) => {
            tracing::debug!(
                "Failed to run script '{script_name}' in workspace '{workspace_name}': {e}"
            );
            WorkspaceOutcome::Ran {
                name: workspace_name.to_string(),
                header,
                body,
                success: false,
            }
        }
    }
}

fn append_captured(buf: &mut Vec<u8>, stdout: &[u8], stderr: &[u8]) {
    for bytes in [stdout, stderr] {
        if !bytes.is_empty() {
            buf.extend_from_slice(bytes);
            if !bytes.ends_with(b"\n") {
                buf.push(b'\n');
            }
        }
    }
}
