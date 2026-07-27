//! Command construction and execution primitives for package scripts.

use std::borrow::Cow;
use std::env;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::ScriptOutput;
use crate::fs;
use crate::model::package::{LifecycleHook, PackageInfo};
use crate::service::binary::get_envs;
use crate::util::format_print::announce_script;
use crate::util::invocation;
use crate::util::platform_const::PATH_SEPARATOR;
use crate::util::user_config::get_install_scope;

/// A consumer for a script's captured output, one call per output segment. The
/// executor stays unaware of *who* consumes the lines (here it's the progress
/// UI's per-script tap), so output capture isn't coupled to the display.
pub(crate) type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Build a `Command` with the standard npm env vars for script execution.
async fn build_script_command(
    package: &PackageInfo,
    script_name: &str,
    script_content: &str,
) -> Result<Command> {
    let bin_paths = ScriptService::collect_bin_paths(package).await?;
    let env_path = ScriptService::build_path_env(&bin_paths);
    let init_cwd =
        env::current_dir().context("failed to read current working directory for INIT_CWD")?;

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(script_content)
        .current_dir(&package.path)
        .env("PATH", env_path)
        .env("npm_lifecycle_event", script_name)
        .env("INIT_CWD", init_cwd)
        .env("npm_package_json", package.path.join("package.json"))
        .env("npm_config_global", get_install_scope().as_env_value());

    if let Some(envs) = get_envs() {
        tracing::debug!(
            "Injecting {} binary mirror envs for {}",
            envs.len(),
            package.name
        );
        for (key, value) in envs {
            cmd.env(key, value);
        }
    }

    // Restore the default SIGPIPE disposition in the child. The parent
    // ignores SIGPIPE (see `crate::util::sysconf`) so its own work survives a
    // broken stdout, but that ignored disposition is inherited across `exec`.
    // A lifecycle script that pipes into an early-closing reader (`script |
    // head`) must instead get normal pipe semantics — die cleanly via SIGPIPE
    // rather than see spurious `EPIPE` write errors that derail a `&&` chain.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
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

/// Map a child `ExitStatus` to the exit code utoo should adopt: `128 + signal`
/// for a signal death (so SIGPIPE → 141), otherwise the child's own code,
/// falling back to 1 when neither is available.
fn status_exit_code(status: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
}

/// Cap one captured stream at 64 KiB in the debug log; a runaway script can
/// emit hundreds of megabytes and the log file is per-run, not rotated.
fn truncate_for_log(bytes: &[u8]) -> Cow<'_, str> {
    const LIMIT: usize = 64 * 1024;
    if bytes.len() <= LIMIT {
        return String::from_utf8_lossy(bytes);
    }
    let mut s = String::from_utf8_lossy(&bytes[..LIMIT]).into_owned();
    s.push_str(&format!("\n… [truncated {} bytes]\n", bytes.len() - LIMIT));
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
    let mut chunk = [0u8; 8 * 1024];
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

pub struct ScriptService;

impl ScriptService {
    pub async fn execute_script(
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

            if Self::is_node_gyp_pkg(package) {
                Self::ensure_node_gyp().await?;
            }

            let mut cmd = build_script_command(package, hook.into(), script).await?;
            tracing::debug!("Executing command: {cmd:?}");

            if output == ScriptOutput::Verbose {
                cmd.stdin(std::process::Stdio::inherit())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());

                let status = tokio::process::Command::from(cmd)
                    .status()
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
                let output = Self::run_captured(cmd, sink.as_ref())
                    .await
                    .context("Failed to execute script")?;

                if !output.status.success() {
                    // Relay the failed script's captured output. Ignore write
                    // errors: the parent keeps SIGPIPE ignored to survive a closed
                    // stdout (see the SIGPIPE handling above), so a plain
                    // `println!`/`eprintln!` here would panic on `BrokenPipe`
                    // rather than letting the install bail cleanly.
                    if !invocation::json() {
                        use std::io::Write as _;
                        if !output.stdout.is_empty() {
                            let _ = writeln!(
                                std::io::stdout(),
                                "{}",
                                String::from_utf8_lossy(&output.stdout)
                            );
                        }
                        if !output.stderr.is_empty() {
                            let _ = writeln!(
                                std::io::stderr(),
                                "{}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                    }

                    anyhow::bail!(
                        "Script execution failed for {hook} in {}:\nCommand: {}\nExit code: {}",
                        package.path.display(),
                        script,
                        output.status.code().unwrap_or(-1)
                    );
                }

                // On success the output is otherwise discarded — keep it in the
                // debug log file so `utoo-*.log` answers "what did that
                // postinstall actually do?" after the fact.
                if !output.stdout.is_empty() || !output.stderr.is_empty() {
                    tracing::debug!(
                        "{hook} output for {}:\n--- stdout ---\n{}--- stderr ---\n{}",
                        package.name,
                        truncate_for_log(&output.stdout),
                        truncate_for_log(&output.stderr),
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
    async fn run_captured(cmd: Command, sink: Option<&OutputSink>) -> Result<std::process::Output> {
        let mut child = tokio::process::Command::from(cmd)
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

    pub async fn ensure_executable(target_path: &Path) -> Result<()> {
        // Early check for file existence (works on all platforms)
        let metadata = crate::fs::metadata(&target_path)
            .await
            .with_context(|| format!("Failed to access file {}", target_path.display()))?;

        if !metadata.is_file() {
            anyhow::bail!("Path is not a file: {}", target_path.display());
        }

        // Unix: only process if not already executable. Windows: always (no
        // executable bit to gate on).
        #[cfg(unix)]
        let needs_shebang = metadata.permissions().mode() & 0o111 == 0;
        #[cfg(not(unix))]
        let needs_shebang = true;

        if needs_shebang {
            Self::try_add_shebang(target_path).await;
        }

        // Set executable permissions on Unix
        #[cfg(unix)]
        {
            let mut perms = crate::fs::metadata(&target_path)
                .await
                .with_context(|| {
                    format!("Failed to get file permissions {}", target_path.display())
                })?
                .permissions();

            perms.set_mode(0o755);
            fs::set_permissions(&target_path, perms)
                .await
                .context("Failed to set executable permissions")?;
        }

        Ok(())
    }

    /// Run [`check_and_add_shebang`](Self::check_and_add_shebang) and log the
    /// outcome. A shebang failure (binary / non-UTF8 file) is non-fatal — the
    /// file just isn't a shell script — so it is logged, not propagated.
    async fn try_add_shebang(target_path: &Path) {
        match Self::check_and_add_shebang(target_path).await {
            Ok(true) => tracing::debug!("Added shebang to {}", target_path.display()),
            Ok(false) => {}
            Err(e) => tracing::debug!("Skipping shebang for {}: {}", target_path.display(), e),
        }
    }

    /// Check if file needs shebang and add it if needed
    /// Returns Ok(true) if shebang was added, Ok(false) if not needed, Err if binary/non-UTF8
    async fn check_and_add_shebang(target_path: &Path) -> Result<bool> {
        // Read first 512 bytes to check for shebang and validate UTF-8
        // file is automatically dropped here
        let header = {
            let mut file = fs::File::open(target_path).await?;
            let mut buffer = vec![0u8; 512];
            let n = file.read(&mut buffer).await?;
            buffer.truncate(n);

            // Try to parse as UTF-8 to detect binary files early
            std::str::from_utf8(&buffer)
                .map_err(|_| anyhow::anyhow!("File is not valid UTF-8, likely a binary file"))?
                .to_string()
        };

        // Check if already has shebang
        if header.starts_with("#!") {
            return Ok(false);
        }

        // Need to add shebang - read entire file now
        let content = fs::read_to_string(target_path).await?;
        let new_content = format!("#!/usr/bin/env node\n{}", content);

        // Write the modified content
        // file is automatically dropped here
        {
            let mut file = fs::File::create(target_path).await?;
            file.write_all(new_content.as_bytes()).await?;
            file.flush().await?;
        }

        Ok(true)
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

    fn build_path_env(bin_paths: &[PathBuf]) -> String {
        let path_separator = PATH_SEPARATOR;
        let original_path = env::var("PATH").unwrap_or_default();
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

        let mut cmd = build_script_command(package, script_name, &cmd_content).await?;
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let status = tokio::process::Command::from(cmd)
            .status()
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
        package: &PackageInfo,
        script_name: &str,
        script_content: &str,
        script_args: Vec<&str>,
    ) -> Result<std::process::Output> {
        let cmd_content = join_script_args(script_content, &script_args);

        let mut cmd = build_script_command(package, script_name, &cmd_content).await?;
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        tokio::process::Command::from(cmd)
            .output()
            .await
            .context("Failed to execute custom script")
    }
}

#[cfg(test)]
mod tests {
    use crate::model::package::LifecycleScripts;

    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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

    #[tokio::test]
    async fn test_ensure_executable() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.sh");
        fs::write(&test_file, "#!/bin/sh\necho test").unwrap();

        // Test ensure_executable
        let result = ScriptService::ensure_executable(&test_file).await;
        assert!(result.is_ok(), "Failed to ensure file is executable");

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&test_file).unwrap().permissions();
            assert!(permissions.mode() & 0o111 != 0, "File not made executable");
        }
    }

    #[tokio::test]
    async fn test_ensure_executable_nonexistent_file() {
        // Test with non-existent file
        let result = ScriptService::ensure_executable(Path::new("nonexistent-file")).await;
        assert!(result.is_err(), "Should fail with non-existent file");
    }

    #[tokio::test]
    async fn test_ensure_executable_binary_file() {
        // Test with a binary file (simulating node executable)
        let temp_dir = TempDir::new().unwrap();
        let binary_file = temp_dir.path().join("node");

        // Create a fake binary file with non-UTF8 bytes
        let binary_data = vec![
            0x7f, 0x45, 0x4c, 0x46, // ELF magic number
            0x02, 0x01, 0x01, 0x00, // 64-bit, little-endian
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFE, 0xFD,
            0xFC, // Some non-UTF8 bytes
        ];
        fs::write(&binary_file, &binary_data).unwrap();

        // Should not fail, just skip shebang and set permissions
        let result = ScriptService::ensure_executable(&binary_file).await;
        assert!(result.is_ok(), "Should handle binary files gracefully");

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&binary_file).unwrap().permissions();
            assert!(
                permissions.mode() & 0o111 != 0,
                "Binary file should be executable"
            );
        }

        // File content should not be modified
        let content = fs::read(&binary_file).unwrap();
        assert_eq!(content, binary_data, "Binary file should not be modified");
    }

    #[tokio::test]
    async fn test_ensure_executable_text_without_shebang() {
        // Test with a text file without shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.js");

        // Create a text file without shebang
        fs::write(&text_file, "console.log('hello');").unwrap();

        let result = ScriptService::ensure_executable(&text_file).await;
        assert!(result.is_ok(), "Should add shebang to text file");

        // Should have shebang added
        let content = fs::read_to_string(&text_file).unwrap();
        assert!(
            content.starts_with("#!/usr/bin/env node\n"),
            "Shebang should be added"
        );
        assert!(
            content.contains("console.log('hello');"),
            "Original content should be preserved"
        );

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&text_file).unwrap().permissions();
            assert!(
                permissions.mode() & 0o111 != 0,
                "Text file should be executable"
            );
        }
    }

    #[tokio::test]
    async fn test_ensure_executable_text_with_shebang() {
        // Test with a text file that already has shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.sh");

        let original_content = "#!/bin/bash\necho 'test'";
        fs::write(&text_file, original_content).unwrap();

        let result = ScriptService::ensure_executable(&text_file).await;
        assert!(result.is_ok(), "Should handle file with existing shebang");

        // Content should not be modified
        let content = fs::read_to_string(&text_file).unwrap();
        assert_eq!(
            content, original_content,
            "File with shebang should not be modified"
        );

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&text_file).unwrap().permissions();
            assert!(permissions.mode() & 0o111 != 0, "File should be executable");
        }
    }

    #[tokio::test]
    async fn test_check_and_add_shebang_binary() {
        // Test check_and_add_shebang with binary file
        let temp_dir = TempDir::new().unwrap();
        let binary_file = temp_dir.path().join("binary");

        // Create binary file
        let binary_data = vec![0xFF, 0xFE, 0xFD, 0xFC];
        fs::write(&binary_file, &binary_data).unwrap();

        let result = ScriptService::check_and_add_shebang(&binary_file).await;
        assert!(result.is_err(), "Should return error for binary file");
        assert!(
            result.unwrap_err().to_string().contains("UTF-8"),
            "Error should mention UTF-8"
        );
    }

    #[tokio::test]
    async fn test_check_and_add_shebang_text_without_shebang() {
        // Test check_and_add_shebang with text file without shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.js");

        fs::write(&text_file, "console.log('test');").unwrap();

        let result = ScriptService::check_and_add_shebang(&text_file).await;
        assert!(result.is_ok(), "Should succeed for text file");
        assert!(result.unwrap(), "Should return true when shebang was added");

        let content = fs::read_to_string(&text_file).unwrap();
        assert!(content.starts_with("#!/usr/bin/env node\n"));
    }

    #[tokio::test]
    async fn test_check_and_add_shebang_text_with_shebang() {
        // Test check_and_add_shebang with text file that already has shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.sh");

        fs::write(&text_file, "#!/bin/sh\necho test").unwrap();

        let result = ScriptService::check_and_add_shebang(&text_file).await;
        assert!(result.is_ok(), "Should succeed for file with shebang");
        assert!(
            !result.unwrap(),
            "Should return false when shebang already exists"
        );
    }
}
