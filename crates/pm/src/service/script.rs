use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::OnceCell;
use tokio::task::JoinSet;

use super::binary::get_envs;
use crate::fs;
use crate::helper::workspace::find_workspace_path;
use crate::model::package::PackageInfo;
use crate::util::format_print::{
    announce_script, print_layer_separator, print_multi_workspace_header, print_workspace_result,
};
use crate::util::platform_const::PATH_SEPARATOR;
use crate::util::user_config::get_install_scope;

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

    if let Some(envs) = get_envs().await {
        tracing::debug!(
            "Injecting {} binary mirror envs for {}",
            envs.len(),
            package.name
        );
        for (key, value) in envs {
            if let Some(value_str) = value.as_str() {
                cmd.env(key, value_str);
            }
        }
    }

    Ok(cmd)
}

/// Cached result of node-gyp availability check and installation
static NODE_GYP_ENSURED: OnceCell<Result<bool, String>> = OnceCell::const_new();

pub struct ScriptService;

impl ScriptService {
    /// Check if node-gyp exists in PATH by searching directories
    fn has_node_gyp_in_path() -> bool {
        let path_separator = PATH_SEPARATOR;
        env::var("PATH").is_ok_and(|paths| {
            paths
                .split(path_separator)
                .map(|dir| Path::new(dir).join("node-gyp"))
                .any(|path| path.exists())
        })
    }

    /// Ensure node-gyp is available in PATH, install globally if not.
    /// Uses OnceCell to ensure installation happens only once, even with concurrent calls.
    pub async fn ensure_node_gyp() -> Result<bool> {
        let result = NODE_GYP_ENSURED
            .get_or_init(|| async {
                if Self::has_node_gyp_in_path() {
                    tracing::debug!("node-gyp found in PATH");
                    return Ok(true);
                }

                tracing::debug!("node-gyp not found in PATH, installing globally");
                let status = Command::new("ut")
                    .args(["i", "-g", "node-gyp"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();

                match status {
                    Ok(s) if s.success() => {
                        tracing::debug!("node-gyp installed successfully");
                        Ok(true)
                    }
                    Ok(s) => Err(format!("Failed to install node-gyp globally: {s}")),
                    Err(e) => Err(format!("Failed to run node-gyp installation: {e}")),
                }
            })
            .await;

        match result {
            Ok(v) => Ok(*v),
            Err(e) => anyhow::bail!("{e}"),
        }
    }

    pub fn is_node_gyp_pkg(package: &PackageInfo) -> bool {
        // https://hitu.antgroup-inc.cn/packages/@npmcli/node-gyp/files/lib/index.js#L6:L6
        package.path.join("binding.gyp").exists()
    }

    pub async fn execute_script(
        package: &PackageInfo,
        hook: crate::model::package::LifecycleHook,
        output: ScriptOutput,
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
                let output = tokio::process::Command::from(cmd)
                    .output()
                    .await
                    .context("Failed to execute script")?;

                if !output.status.success() {
                    if !output.stdout.is_empty() {
                        println!("{}", String::from_utf8_lossy(&output.stdout));
                    }
                    if !output.stderr.is_empty() {
                        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
                    }

                    anyhow::bail!(
                        "Script execution failed for {hook} in {}:\nCommand: {}\nExit code: {}",
                        package.path.display(),
                        script,
                        output.status.code().unwrap_or(-1)
                    );
                }
            }
        }

        Ok(())
    }

    pub async fn ensure_executable(target_path: &Path) -> Result<()> {
        // Early check for file existence (works on all platforms)
        let metadata = crate::fs::metadata(&target_path)
            .await
            .with_context(|| format!("Failed to access file {}", target_path.display()))?;

        if !metadata.is_file() {
            anyhow::bail!("Path is not a file: {}", target_path.display());
        }

        // Unix: Only process if not already executable
        #[cfg(unix)]
        {
            let permissions = metadata.permissions();
            let is_executable = permissions.mode() & 0o111 != 0;

            if !is_executable {
                match Self::check_and_add_shebang(target_path).await {
                    Ok(added) => {
                        if added {
                            tracing::debug!("Added shebang to {}", target_path.display());
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Skipping shebang for {}: {}", target_path.display(), e);
                    }
                }
            }
        }

        // Windows: Always try to add shebang (no executable bit to check)
        #[cfg(not(unix))]
        {
            match Self::check_and_add_shebang(target_path).await {
                Ok(added) => {
                    if added {
                        tracing::debug!("Added shebang to {}", target_path.display());
                    }
                }
                Err(e) => {
                    tracing::debug!("Skipping shebang for {}: {}", target_path.display(), e);
                }
            }
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

        let cmd_content = match script_args.is_empty() {
            true => script_content,
            false => &format!("{} {}", script_content, script_args.join(" ")),
        };

        let mut cmd = build_script_command(package, script_name, cmd_content).await?;
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let status = tokio::process::Command::from(cmd)
            .status()
            .await
            .context("Failed to execute custom script")?;

        if !status.success() {
            anyhow::bail!(
                "Custom script execution failed with exit code: {}",
                status.code().unwrap_or(-1)
            );
        }

        Ok(())
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

    /// Like [`Self::execute_custom_script`], but captures stdout/stderr
    /// instead of streaming to the terminal.
    pub async fn execute_custom_script_captured(
        package: &PackageInfo,
        script_name: &str,
        script_content: &str,
        script_args: Vec<&str>,
    ) -> Result<std::process::Output> {
        let cmd_content = if script_args.is_empty() {
            script_content
        } else {
            &format!("{} {}", script_content, script_args.join(" "))
        };

        let mut cmd = build_script_command(package, script_name, cmd_content).await?;
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        tokio::process::Command::from(cmd)
            .output()
            .await
            .context("Failed to execute custom script")
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

#[cfg(test)]
mod tests {
    use crate::model::package::LifecycleScripts;

    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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

    #[test]
    fn test_has_node_gyp_in_path_found() {
        // Create a temporary directory
        let temp_dir = tempdir().unwrap();
        let node_gyp_path = temp_dir.path().join("node-gyp");
        // Create a dummy node-gyp executable
        fs::write(&node_gyp_path, "#!/bin/sh\necho node-gyp").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&node_gyp_path, fs::Permissions::from_mode(0o755)).unwrap();

        // Save original PATH
        let original_path = env::var("PATH").unwrap_or_default();
        // Set PATH to only include our temp dir
        unsafe {
            env::set_var("PATH", temp_dir.path());
        }

        // Should find node-gyp
        assert!(ScriptService::has_node_gyp_in_path());

        // Restore original PATH
        unsafe {
            env::set_var("PATH", original_path);
        }
    }

    #[tokio::test]
    async fn test_ensure_node_gyp_found() {
        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let node_gyp_path = temp_dir.path().join("node-gyp");
        // Create a dummy node-gyp executable
        fs::write(&node_gyp_path, "#!/bin/sh\necho node-gyp").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&node_gyp_path, fs::Permissions::from_mode(0o755)).unwrap();

        // Save original PATH
        let original_path = env::var("PATH").unwrap_or_default();
        // Set PATH to only include our temp dir
        unsafe {
            env::set_var("PATH", temp_dir.path());
        }
        // Should return Ok(true) because node-gyp exists
        let result = ScriptService::ensure_node_gyp().await;
        assert!(result.is_ok());
        assert!(result.unwrap());
        // Restore original PATH
        unsafe {
            env::set_var("PATH", original_path);
        }
    }

    #[tokio::test]
    async fn test_is_node_gyp_pkg_true_and_false() {
        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let package_path = temp_dir.path();
        // Case 1: binding.gyp exists
        let binding_gyp = package_path.join("binding.gyp");
        fs::write(&binding_gyp, "{}").unwrap();
        let package = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: Default::default(),
            scripts: Default::default(),
            lifecycle_scripts: LifecycleScripts::default(),
            name: "test-package".to_string(),
        };
        assert!(ScriptService::is_node_gyp_pkg(&package));
        // Case 2: binding.gyp does not exist
        fs::remove_file(&binding_gyp).unwrap();
        assert!(!ScriptService::is_node_gyp_pkg(&package));
    }
}
