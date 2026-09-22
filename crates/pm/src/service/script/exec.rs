//! Command construction and execution primitives for package scripts.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write as _};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;

use super::ScriptOutput;
use crate::model::cli_output::{
    CAPTURED_OUTPUT_TAIL_LIMIT, CapturedOutput, ErrorDetails, ExecutionStatus, LifecycleExecution,
};
use crate::model::package::{LifecycleHook, PackageInfo};
use crate::util::cli_enum::InstallScope;
use crate::util::format_print::announce_script;
use crate::util::platform_const::PATH_SEPARATOR;

/// A consumer for a script's captured output, one call per output segment. The
/// executor stays unaware of *who* consumes the lines (here it's the progress
/// UI's per-script tap), so output capture isn't coupled to the display.
pub(crate) type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Build a `Command` with the standard npm env vars for script execution.
async fn build_script_command(
    environment: &ScriptEnvironment,
    tools: &PreparedTools,
    package: &PackageInfo,
    script_name: &str,
    script_content: &str,
) -> Result<Command> {
    let mut bin_paths = ScriptService::collect_bin_paths(package).await?;
    bin_paths.extend(tools.bin_dirs.iter().cloned());
    let env_path = ScriptService::build_path_env(&bin_paths, &environment.path);

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(script_content)
        .current_dir(&package.path)
        .env("PATH", env_path)
        .env("npm_lifecycle_event", script_name)
        .env("INIT_CWD", &environment.init_cwd)
        .env("npm_package_json", package.path.join("package.json"))
        .env("npm_config_global", environment.scope.as_env_value());

    cmd.envs(&environment.extra);
    if let Some(node_gyp) = &tools.node_gyp {
        cmd.env("npm_config_node_gyp", node_gyp);
    }

    // Restore the default SIGPIPE disposition in the child. The parent
    // ignores SIGPIPE (see `crate::util::sysconf`) so its own work survives a
    // broken stdout, but that ignored disposition is inherited across `exec`.
    // A lifecycle script that pipes into an early-closing reader (`script |
    // head`) must instead get normal pipe semantics — die cleanly via SIGPIPE
    // rather than see spurious `EPIPE` write errors that derail a `&&` chain.
    #[cfg(unix)]
    {
        // SAFETY: the closure runs in the forked child before `exec` and only
        // calls `signal(2)`, which is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);
                Ok(())
            });
        }
    }

    Ok(cmd)
}

/// A package script exited unsuccessfully. Carries the exit code utoo should
/// terminate with, so `utoo run <script>` faithfully mirrors the script's own
/// status: a non-zero `exit N` propagates as `N`, and a signal death (e.g.
/// SIGPIPE from `script | head`) propagates as `128 + N` — matching npm/pnpm
/// and shell convention.
#[derive(Debug)]
pub struct ScriptExit {
    pub code: i32,
    message: String,
}

impl std::fmt::Display for ScriptExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ScriptExit {}

/// A captured lifecycle script failure with machine-readable diagnostics.
///
/// The display text preserves the existing human error chain, while `details`
/// exposes bounded output tails to JSON callers without producing a second
/// stdout/stderr document.
#[derive(Debug)]
pub(crate) struct ScriptFailure {
    message: String,
    execution: LifecycleExecution,
}

impl ScriptFailure {
    pub(crate) fn failed_to_start(
        package: &PackageInfo,
        event: &str,
        command: String,
        duration: Duration,
        error: &anyhow::Error,
    ) -> Self {
        Self {
            message: format!("Failed to execute {event}: {error:#}"),
            execution: LifecycleExecution {
                package: (!package.name.is_empty()).then(|| package.name.clone()),
                workspace: None,
                event: event.to_string(),
                command,
                cwd: package.path.to_string_lossy().into_owned(),
                status: ExecutionStatus::FailedToStart,
                exit_code: None,
                stdout: CapturedOutput::empty(),
                stderr: CapturedOutput::empty(),
                duration_ms: duration.as_millis() as u64,
            },
        }
    }

    fn new(
        message: String,
        package: &PackageInfo,
        event: impl Into<String>,
        command: impl Into<String>,
        output: &std::process::Output,
        duration: Duration,
    ) -> Self {
        Self {
            message,
            execution: LifecycleExecution {
                package: (!package.name.is_empty()).then(|| package.name.clone()),
                workspace: None,
                event: event.into(),
                command: command.into(),
                cwd: package.path.to_string_lossy().into_owned(),
                status: ExecutionStatus::Failed,
                exit_code: Some(status_exit_code(&output.status) as u32),
                stdout: CapturedOutput::from_bytes(&output.stdout),
                stderr: CapturedOutput::from_bytes(&output.stderr),
                duration_ms: duration.as_millis() as u64,
            },
        }
    }

    pub(crate) fn lifecycle(
        package: &PackageInfo,
        event: &str,
        command: &str,
        args: &[&str],
        output: &std::process::Output,
        duration: Duration,
    ) -> Self {
        let exit_code = status_exit_code(&output.status);
        Self::new(
            format!("Failed to execute {event}: exit code {exit_code}"),
            package,
            event,
            join_script_args(command, args),
            output,
            duration,
        )
    }

    fn dependency(
        package: &PackageInfo,
        event: LifecycleHook,
        command: &str,
        output: &std::process::Output,
        duration: Duration,
    ) -> Self {
        // Preserve the existing human-facing message exactly. The structured
        // `exitCode` still uses shell-compatible signal mapping.
        let display_exit_code = output.status.code().unwrap_or(-1);
        Self::new(
            format!(
                "Script execution failed for {event} in {}:\nCommand: {command}\nExit code: {display_exit_code}",
                package.path.display()
            ),
            package,
            event.to_string(),
            command,
            output,
            duration,
        )
    }

    fn details(&self) -> ErrorDetails {
        ErrorDetails::Lifecycle {
            executions: vec![self.execution.clone()],
        }
    }
}

impl std::fmt::Display for ScriptFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ScriptFailure {}

pub(crate) fn script_failure_details(error: &anyhow::Error) -> Option<ErrorDetails> {
    error.chain().find_map(|source| {
        source
            .downcast_ref::<ScriptFailure>()
            .map(ScriptFailure::details)
    })
}

/// Map a child `ExitStatus` to the exit code utoo should adopt: `128 + signal`
/// for a signal death (so SIGPIPE → 141), otherwise the child's own code,
/// falling back to 1 when neither is available.
pub(crate) fn status_exit_code(status: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
}

/// Cap one captured stream at 64 KiB in the debug log; a runaway script can
/// emit hundreds of megabytes and the log file is per-run, not rotated.
fn truncate_for_log(bytes: &[u8]) -> Cow<'_, str> {
    if bytes.len() <= CAPTURED_OUTPUT_TAIL_LIMIT {
        return String::from_utf8_lossy(bytes);
    }
    let mut s = String::from_utf8_lossy(&bytes[..CAPTURED_OUTPUT_TAIL_LIMIT]).into_owned();
    s.push_str(&format!(
        "\n… [truncated {} bytes]\n",
        bytes.len() - CAPTURED_OUTPUT_TAIL_LIMIT
    ));
    Cow::Owned(s)
}

/// Append extra CLI args to a script body, borrowing when there are none —
/// the common per-script case allocates nothing.
fn join_script_args<'a>(script_content: &'a str, script_args: &[&str]) -> Cow<'a, str> {
    if script_args.is_empty() {
        Cow::Borrowed(script_content)
    } else {
        Cow::Owned(format!("{} {}", script_content, script_args.join(" ")))
    }
}

/// Read a child pipe to EOF, returning its raw bytes while feeding each output
/// segment to `sink`. Splits on both `\n` and `\r` so a `\r`-updated progress
/// bar (e.g. puppeteer's Chromium download) still surfaces its latest state, not
/// just whole newline-terminated lines.
async fn drain_tapped<R: tokio::io::AsyncRead + Unpin>(
    reader: Option<R>,
    sink: Option<OutputSink>,
) -> Vec<u8> {
    let mut raw = Vec::new();
    let Some(mut reader) = reader else {
        return raw;
    };
    // The current line, accumulated only for the one-line `↳` preview. Bounded
    // so a script that emits a huge line (or binary) with no `\n`/`\r` can't grow
    // it without limit — the full bytes still land in `raw` for the dump.
    const MAX_SEGMENT: usize = 4 * 1024;
    let mut segment: Vec<u8> = Vec::new();
    // Keep pipe buffers out of the nested installation/lifecycle futures.
    // Inline arrays inflate every caller and exhaust the Windows debug stack.
    let mut chunk = vec![0u8; 8 * 1024];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                for &byte in &chunk[..n] {
                    if byte == b'\n' || byte == b'\r' {
                        emit_segment(&segment, sink.as_ref());
                        segment.clear();
                    } else if segment.len() < MAX_SEGMENT {
                        segment.push(byte);
                    }
                }
            }
            // Keep what was read so far, but don't pretend it was a clean EOF —
            // a truncated capture would otherwise silently weaken a failing
            // script's diagnostic dump.
            Err(e) => {
                tracing::debug!("script output pipe read failed (output truncated): {e}");
                break;
            }
        }
    }
    emit_segment(&segment, sink.as_ref());
    raw
}

/// Forward a non-blank output segment to `sink` as the script's latest line.
fn emit_segment(segment: &[u8], sink: Option<&OutputSink>) {
    let Some(sink) = sink else { return };
    let text = String::from_utf8_lossy(segment);
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        sink(trimmed);
    }
}

/// npm environment supplied by the operation that owns script execution.
#[derive(Clone)]
pub struct ScriptEnvironment {
    pub init_cwd: PathBuf,
    pub path: OsString,
    pub scope: InstallScope,
    pub extra: BTreeMap<String, String>,
    pub prefix: Option<String>,
}

/// Tool paths prepared by install/pack/publish before invoking a hook.
#[derive(Clone, Default, Debug)]
pub struct PreparedTools {
    pub node_gyp: Option<PathBuf>,
    pub bin_dirs: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct ScriptService {
    environment: Arc<ScriptEnvironment>,
    tools: PreparedTools,
}

impl ScriptService {
    pub fn new(environment: ScriptEnvironment) -> Self {
        Self {
            environment: Arc::new(environment),
            tools: PreparedTools::default(),
        }
    }

    pub fn environment(&self) -> &ScriptEnvironment {
        &self.environment
    }
    pub fn tools(&self) -> &PreparedTools {
        &self.tools
    }
    pub fn with_tools(&self, tools: PreparedTools) -> Self {
        Self {
            environment: self.environment.clone(),
            tools,
        }
    }
    pub fn with_prefix(&self, prefix: Option<&str>) -> Self {
        let mut environment = (*self.environment).clone();
        environment.prefix = prefix.map(str::to_owned);
        Self {
            environment: Arc::new(environment),
            tools: self.tools.clone(),
        }
    }

    pub async fn execute_script(
        &self,
        package: &PackageInfo,
        hook: LifecycleHook,
        output: ScriptOutput,
        sink: Option<OutputSink>,
    ) -> Result<()> {
        let script = package.lifecycle_scripts.get_script(hook);

        if let Some(script) = script {
            tracing::debug!(
                "Executing {hook} script for {}: {}",
                package.path.display(),
                script
            );

            if output == ScriptOutput::Verbose {
                announce_script(None, script, "");
            }

            let mut cmd =
                build_script_command(&self.environment, &self.tools, package, hook.into(), script)
                    .await?;
            tracing::debug!("Executing command: {cmd:?}");

            if output == ScriptOutput::Verbose {
                cmd.stdin(std::process::Stdio::inherit())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());

                let status = Self::run_inherited(cmd)
                    .await
                    .context("Failed to execute script")?;

                if !status.success() {
                    anyhow::bail!(
                        "Script execution failed for {hook} in {}: exit code {}",
                        package.path.display(),
                        status.code().unwrap_or(-1)
                    );
                }
            } else {
                // Pipe and drain line-by-line rather than buffering with
                // `.output()`: each line feeds `sink` so the long-run heartbeat
                // can show what a slow, silent script is doing, while the full
                // text is still collected for the failure dump / debug log.
                let started = std::time::Instant::now();
                let captured = Self::run_captured(cmd, sink.as_ref())
                    .await
                    .context("Failed to execute script")?;

                if !captured.status.success() {
                    // Relay the failed script's captured output. Ignore write
                    // errors: the parent keeps SIGPIPE ignored to survive a closed
                    // stdout (see the SIGPIPE handling above), so a plain
                    // `println!`/`eprintln!` here would panic on `BrokenPipe`
                    // rather than letting the install bail cleanly.
                    if output != ScriptOutput::Machine {
                        if !captured.stdout.is_empty()
                            && let Err(error) = writeln!(
                                io::stdout(),
                                "{}",
                                String::from_utf8_lossy(&captured.stdout)
                            )
                        {
                            tracing::debug!("failed to relay script stdout: {error}");
                        }
                        if !captured.stderr.is_empty()
                            && let Err(error) = writeln!(
                                io::stderr(),
                                "{}",
                                String::from_utf8_lossy(&captured.stderr)
                            )
                        {
                            tracing::debug!("failed to relay script stderr: {error}");
                        }
                    }

                    return Err(ScriptFailure::dependency(
                        package,
                        hook,
                        script,
                        &captured,
                        started.elapsed(),
                    )
                    .into());
                }

                // On success the output is otherwise discarded — keep it in the
                // debug log file so `utoo-*.log` answers "what did that
                // postinstall actually do?" after the fact.
                if !captured.stdout.is_empty() || !captured.stderr.is_empty() {
                    tracing::debug!(
                        "{hook} output for {}:\n--- stdout ---\n{}--- stderr ---\n{}",
                        package.name,
                        truncate_for_log(&captured.stdout),
                        truncate_for_log(&captured.stderr),
                    );
                }
            }
        }

        Ok(())
    }

    /// Run `cmd` with stdout/stderr piped, draining both concurrently so the
    /// pipes can't fill and deadlock the child. Each output segment is fed to
    /// `sink` (for the long-run heartbeat) while the raw bytes are collected into
    /// a [`std::process::Output`], so callers keep the same failure-dump /
    /// debug-log behaviour they had with `.output()`.
    pub(crate) async fn run_inherited(mut cmd: Command) -> Result<std::process::ExitStatus> {
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        Ok(tokio::process::Command::from(cmd)
            .kill_on_drop(true)
            .status()
            .await?)
    }

    pub(crate) async fn run_captured(
        cmd: Command,
        sink: Option<&OutputSink>,
    ) -> Result<std::process::Output> {
        let mut child = tokio::process::Command::from(cmd)
            .kill_on_drop(true)
            // Null stdin, matching the replaced `.output()`: a dependency script
            // that reads stdin must get immediate EOF, not inherit the user's
            // terminal — otherwise it blocks forever waiting for input it'll
            // never get, hanging the install (and holding a concurrency slot).
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn script")?;

        // Drain both pipes and wait for the child concurrently on this one task
        // (no `tokio::spawn`): the drains keep the pipes from filling while
        // `wait` runs, and joining in place avoids the spawn overhead and the
        // `JoinError` path entirely. Take the pipe handles first so the only
        // borrow of `child` inside `join!` is `wait`.
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let (status, stdout, stderr) = tokio::join!(
            child.wait(),
            drain_tapped(stdout_pipe, sink.cloned()),
            drain_tapped(stderr_pipe, sink.cloned()),
        );

        Ok(std::process::Output {
            status: status.context("Failed to wait for script")?,
            stdout,
            stderr,
        })
    }

    async fn collect_bin_paths(package: &PackageInfo) -> Result<Vec<PathBuf>> {
        let mut bin_paths = Vec::new();
        let mut current_path = Some(package.path.as_path());

        while let Some(path) = current_path {
            let bin_path = path.join("node_modules/.bin");
            if crate::fs::try_exists(&bin_path).await?
                && let Ok(absolute_path) = crate::fs::canonicalize(&bin_path).await
            {
                bin_paths.push(absolute_path);
            }
            current_path = path.parent();
        }

        Ok(bin_paths)
    }

    fn build_path_env(bin_paths: &[PathBuf], original: &std::ffi::OsStr) -> String {
        let path_separator = PATH_SEPARATOR;
        let original_path = original.to_string_lossy();
        let additional_paths = bin_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(path_separator);

        format!(
            "{}{}{}",
            additional_paths,
            if additional_paths.is_empty() {
                ""
            } else {
                path_separator
            },
            original_path
        )
    }

    pub async fn execute_custom_script(
        &self,
        package: &PackageInfo,
        script_name: &str,
        script_content: &str,
        script_args: Vec<&str>,
    ) -> Result<()> {
        tracing::debug!(
            "Executing custom script for {}: {}",
            package.path.display(),
            script_name
        );

        let cmd_content = join_script_args(script_content, &script_args);

        let mut cmd = build_script_command(
            &self.environment,
            &self.tools,
            package,
            script_name,
            &cmd_content,
        )
        .await?;
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let status = Self::run_inherited(cmd)
            .await
            .context("Failed to execute custom script")?;

        if !status.success() {
            let code = status_exit_code(&status);
            return Err(ScriptExit {
                code,
                message: format!("Custom script execution failed with exit code: {code}"),
            }
            .into());
        }

        Ok(())
    }

    /// Like [`Self::execute_custom_script`], but captures stdout/stderr
    /// instead of streaming to the terminal.
    pub async fn execute_custom_script_captured(
        &self,
        package: &PackageInfo,
        script_name: &str,
        script_content: &str,
        script_args: Vec<&str>,
    ) -> Result<std::process::Output> {
        let cmd_content = join_script_args(script_content, &script_args);

        let cmd = build_script_command(
            &self.environment,
            &self.tools,
            package,
            script_name,
            &cmd_content,
        )
        .await?;
        Self::run_captured(cmd, None)
            .await
            .context("Failed to execute custom script")
    }
}

#[cfg(test)]
mod tests {
    use crate::model::package::LifecycleScripts;

    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn executor_uses_supplied_npm_environment() {
        let root = tempdir().unwrap();
        let initial = tempdir().unwrap();
        let package = PackageInfo {
            path: root.path().to_path_buf(),
            name: "fixture".into(),
            bin_files: vec![],
            scripts: Default::default(),
            lifecycle_scripts: Default::default(),
        };
        let executor = ScriptService::new(ScriptEnvironment {
            init_cwd: initial.path().to_path_buf(),
            path: std::env::var_os("PATH").unwrap_or_default(),
            scope: InstallScope::Global,
            prefix: None,
            extra: BTreeMap::from([("PM_TEST_ENV".into(), "provided".into())]),
        });
        let result = executor.execute_custom_script_captured(&package, "test", r#"node -e "process.stdout.write(JSON.stringify([process.cwd(),process.env.INIT_CWD,process.env.npm_lifecycle_event,process.env.npm_config_global,process.env.PM_TEST_ENV]))""#, vec![]).await.unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let values: Vec<String> = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            std::fs::canonicalize(&values[0]).unwrap(),
            std::fs::canonicalize(root.path()).unwrap()
        );
        assert_eq!(values[1], initial.path().to_string_lossy());
        assert_eq!(&values[2..], ["test", "true", "provided"]);
    }

    #[tokio::test]
    async fn captured_process_drains_both_full_pipes_before_waiting() {
        let mut command = Command::new("node");
        command.args(["-e", "process.stdout.write(Buffer.alloc(262144,65));process.stderr.write(Buffer.alloc(262144,66))"]);
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            ScriptService::run_captured(command, None),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout, vec![65; 262144]);
        assert_eq!(result.stderr, vec![66; 262144]);
    }

    #[cfg(unix)]
    #[test]
    fn test_status_exit_code_signal_and_code() {
        use std::os::unix::process::ExitStatusExt;
        // Killed by SIGPIPE (13) → 128 + 13 = 141, so `script | head` deaths
        // propagate the conventional 141 instead of collapsing to 1.
        let by_signal = std::process::ExitStatus::from_raw(13);
        assert_eq!(status_exit_code(&by_signal), 141);
        // Normal exit with code 7 → 7 (raw wait status encodes it in bits 8..).
        let by_code = std::process::ExitStatus::from_raw(7 << 8);
        assert_eq!(status_exit_code(&by_code), 7);
    }

    #[test]
    fn machine_output_keeps_a_bounded_tail() {
        let mut output = b"BEGIN_MARKER".to_vec();
        output.resize(CAPTURED_OUTPUT_TAIL_LIMIT + 16, b'x');
        output.extend_from_slice(b"END_MARKER");

        let tail = CapturedOutput::from_bytes(&output);

        assert!(tail.truncated);
        assert_eq!(tail.tail.len(), CAPTURED_OUTPUT_TAIL_LIMIT);
        assert!(!tail.tail.contains("BEGIN_MARKER"));
        assert!(tail.tail.ends_with("END_MARKER"));
    }

    #[tokio::test]
    async fn test_collect_bin_paths_with_local_node_modules() {
        let temp_dir = tempdir().unwrap();
        let package_path = temp_dir.path();

        // Create package.json
        let package_json = package_path.join("package.json");
        fs::write(&package_json, "{}").unwrap();

        // Create local node_modules/.bin directory
        let local_bin_dir = package_path.join("node_modules/.bin");
        fs::create_dir_all(&local_bin_dir).unwrap();

        // Create a dummy executable
        let dummy_bin = local_bin_dir.join("test-bin");
        fs::write(&dummy_bin, "#!/bin/sh\necho 'test'").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&dummy_bin, fs::Permissions::from_mode(0o755)).unwrap();

        let package = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: Default::default(),
            scripts: Default::default(),
            lifecycle_scripts: LifecycleScripts::default(),
            name: "test-package".to_string(),
        };

        let bin_paths = ScriptService::collect_bin_paths(&package).await.unwrap();
        assert!(!bin_paths.is_empty());
        assert!(bin_paths[0].ends_with("node_modules/.bin"));
    }
}
