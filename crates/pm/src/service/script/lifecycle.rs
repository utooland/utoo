//! npm lifecycle orchestration: pre/post event chains, single-package runs,
//! and multi-workspace layered execution.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::task::JoinSet;

use super::ScriptService;
use crate::helper::workspace::find_workspace_path;
use crate::model::package::PackageInfo;
use crate::util::format_print::{
    announce_script, print_layer_separator, print_multi_workspace_header, print_workspace_result,
};

/// How script output is handled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutput {
    /// Stream to terminal in real time (user-facing scripts).
    Verbose,
    /// Capture and only print on failure (dependency lifecycle scripts).
    Silent,
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
    },
    Capture {
        workspace_label: &'a str,
        header: &'a mut String,
        body: &'a mut Vec<u8>,
    },
}

impl ScriptService {
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
                LifecycleSink::Stream { workspace_label } => {
                    announce_script(*workspace_label, &content, &step_args.join(" "));
                    Self::execute_custom_script(package, step_name, &content, step_args.to_vec())
                        .await
                        .with_context(|| format!("Failed to execute {step_name}"))?;
                }
                LifecycleSink::Capture {
                    workspace_label,
                    header,
                    body,
                } => {
                    let _ = writeln!(
                        header,
                        "[{}] {} {}",
                        workspace_label,
                        content,
                        step_args.join(" ")
                    );
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
            let workspaces_to_run: Vec<_> = layer
                .iter()
                .filter_map(|name| paths.get(name).map(|p| (name.clone(), p.clone())))
                .collect();

            if workspaces_to_run.is_empty() {
                continue;
            }

            let mut join_set = JoinSet::new();
            for (workspace_name, ws_path) in workspaces_to_run {
                let script_name = script_name.to_string();
                let script_args = script_args.clone();

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
            anyhow::ensure!(
                failed_names.is_empty(),
                "Script execution failed in layer {}: {}",
                layer_index + 1,
                failed_names.join(", ")
            );
        }

        Ok(())
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
