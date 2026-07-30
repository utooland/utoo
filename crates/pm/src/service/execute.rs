use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Result, anyhow};

use crate::error::{CliError, ErrorKind};
use crate::model::cli_output::{
    CapturedOutput, ErrorDetails, ExecutableSource, ExecuteResult, ExecutionStatus,
    ProcessExecution,
};
use crate::service::package_management::PackageManagementService;
use crate::service::script::ScriptOutput;
use crate::util::invocation;
use crate::util::presenter::emit;

/// Execute a package binary
pub async fn execute_package(command: &str, args: Vec<String>) -> Result<()> {
    tracing::debug!("Executing command: {command} with args: {args:?}");

    // First, try to find the binary in local node_modules/.bin directories
    if let Some(binary_path) = find_binary(command).await? {
        tracing::debug!("Found binary at: {}", binary_path.display());
        return execute_binary(command, &binary_path, args, ExecutableSource::Local).await;
    }

    // If not found locally, try to install the package to cache
    tracing::debug!("Command '{command}' not found locally");

    // For now, assume the package name is the same as the command
    // This can be enhanced later to handle more complex package/command mappings
    // command is must be a valid package name
    let package_name = command;

    // Install the package to cache
    let output = if invocation::json() {
        ScriptOutput::Machine
    } else {
        ScriptOutput::Verbose
    };
    let package_cache_dir =
        PackageManagementService::install_package_to_cache(package_name, output).await?;

    // Try to find the binary in the cached package
    // utoo -x eslint --version
    // utoo -x @modelcontextprotocol/create-server --version
    // utoo -x @modelcontextprotocol/create-server create-mcp-server --version
    match find_binary_in_cache(&package_cache_dir).await {
        Ok(Some(binary_path)) => {
            tracing::debug!("Found binary in cache at: {}", binary_path.display());
            execute_binary(command, &binary_path, args, ExecutableSource::Cache).await
        }
        Ok(None) => {
            tracing::error!("No executable found in bin directory for package '{package_name}'");
            tracing::debug!(
                "The package might not provide any executables, or the bin directory might be empty",
            );
            Err(anyhow!("No executable found for package '{package_name}'"))
        }
        Err(e) => {
            tracing::error!("Error finding binary for package '{package_name}': {e}");
            Err(e)
        }
    }
}

/// Execute the binary with given arguments
async fn execute_binary(
    requested: &str,
    binary_path: &Path,
    args: Vec<String>,
    source: ExecutableSource,
) -> Result<()> {
    // On Windows, prefer .cmd shim if it exists, otherwise use sh to execute Unix shim
    let (mut cmd, command_prefix) = {
        #[cfg(windows)]
        {
            let cmd_path = binary_path.with_extension("cmd");
            if cmd_path.exists() {
                (
                    Command::new(&cmd_path),
                    vec![cmd_path.to_string_lossy().into_owned()],
                )
            } else {
                let mut c = Command::new("sh");
                c.arg(binary_path);
                (
                    c,
                    vec!["sh".to_string(), binary_path.to_string_lossy().into_owned()],
                )
            }
        }
        #[cfg(not(windows))]
        {
            (
                Command::new(binary_path),
                vec![binary_path.to_string_lossy().into_owned()],
            )
        }
    };
    cmd.args(&args);

    if invocation::json() {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let cwd = std::env::current_dir()?;
        let command = command_prefix
            .into_iter()
            .chain(args)
            .collect::<Vec<_>>()
            .join(" ");
        let started = Instant::now();
        let output = match cmd.output() {
            Ok(output) => output,
            Err(error) => {
                let execution = ProcessExecution {
                    command,
                    cwd: cwd.to_string_lossy().into_owned(),
                    status: ExecutionStatus::FailedToStart,
                    exit_code: None,
                    stdout: CapturedOutput::empty(),
                    stderr: CapturedOutput::empty(),
                    duration_ms: started.elapsed().as_millis() as u64,
                };
                return Err(CliError::new(
                    ErrorKind::Local,
                    format!("failed to start {requested}: {error}"),
                )
                .with_code("process_start_failed")
                .with_details(ErrorDetails::Process { execution })
                .into());
            }
        };
        let status = if output.status.success() {
            ExecutionStatus::Succeeded
        } else {
            ExecutionStatus::Failed
        };
        let execution = ProcessExecution {
            command,
            cwd: cwd.to_string_lossy().into_owned(),
            status,
            exit_code: Some(status_exit_code(&output.status) as u32),
            stdout: CapturedOutput::from_bytes(&output.stdout),
            stderr: CapturedOutput::from_bytes(&output.stderr),
            duration_ms: started.elapsed().as_millis() as u64,
        };
        if !output.status.success() {
            return Err(CliError::new(
                ErrorKind::Local,
                format!(
                    "command failed with exit code {}",
                    execution.exit_code.unwrap_or(1)
                ),
            )
            .with_code("process_failed")
            .with_details(ErrorDetails::Process { execution })
            .into());
        }
        emit(
            "execute",
            &ExecuteResult {
                requested: requested.to_string(),
                source,
                executable: binary_path.to_string_lossy().into_owned(),
                execution,
            },
            || Ok(()),
        )
    } else {
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = cmd.status()?;

        if status.success() {
            Ok(())
        } else {
            let exit_code = status.code().unwrap_or(-1);
            Err(anyhow!("Command failed with exit code: {exit_code}"))
        }
    }
}

fn status_exit_code(status: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
}

/// Find `command` in `node_modules/.bin`, searching up from the cwd — the
/// standard mechanism for running a locally-installed tool's bin shim.
async fn find_binary(command: &str) -> Result<Option<PathBuf>> {
    let current_dir = std::env::current_dir()?;
    find_binary_in_hierarchy(&current_dir, command).await
}

/// Find the binary of a tool freshly installed into the utx cache: return the
/// first file in `<package_cache_dir>/bin`.
async fn find_binary_in_cache(package_cache_dir: &Path) -> Result<Option<PathBuf>> {
    let bin_dir = package_cache_dir.join("bin");

    if !crate::fs::try_exists(&bin_dir).await? {
        return Ok(None);
    }

    // Get the first file in bin directory
    let entries = fs::read_dir(&bin_dir)?;

    for entry in entries {
        let entry = entry?;
        let path: PathBuf = entry.path();
        if path.is_file() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// Search for binary in the directory hierarchy starting from the given path.
async fn find_binary_in_hierarchy(start_path: &Path, command: &str) -> Result<Option<PathBuf>> {
    let mut current_path = start_path.to_path_buf();

    loop {
        let bin_path = current_path.join("node_modules").join(".bin").join(command);

        // Check if the binary exists and is executable
        if crate::fs::try_exists(&bin_path).await? {
            return Ok(Some(bin_path));
        }

        // Move up to parent directory
        if let Some(parent) = current_path.parent() {
            current_path = parent.to_path_buf();
        } else {
            // Reached the root directory
            break;
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;
    use crate::fs as async_fs;

    // Helper function to create a test directory structure
    async fn create_test_structure() -> Result<TempDir> {
        let temp_dir = TempDir::new()?;
        let base_path = temp_dir.path();

        // Create node_modules/.bin directory
        let bin_dir = base_path.join("node_modules").join(".bin");
        async_fs::create_dir_all(&bin_dir).await?;

        // Create a test binary file
        let test_binary = bin_dir.join("test-cmd");
        async_fs::write(&test_binary, "#!/bin/bash\necho 'test'").await?;

        // Set executable permissions on Unix systems
        #[cfg(unix)]
        {
            let mut perms = async_fs::metadata(&test_binary).await?.permissions();
            perms.set_mode(0o755);
            async_fs::set_permissions(&test_binary, perms).await?;
        }

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn test_find_binary_in_hierarchy_found() {
        let temp_dir = create_test_structure().await.unwrap();
        let start_path = temp_dir.path();

        let result = find_binary_in_hierarchy(start_path, "test-cmd")
            .await
            .unwrap();
        assert!(result.is_some());

        let found_path = result.unwrap();
        assert!(found_path.ends_with("node_modules/.bin/test-cmd"));
        assert!(found_path.exists());
    }

    #[tokio::test]
    async fn test_find_binary_in_hierarchy_not_found() {
        let temp_dir = create_test_structure().await.unwrap();
        let start_path = temp_dir.path();

        let result = find_binary_in_hierarchy(start_path, "nonexistent-cmd")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_find_binary_in_hierarchy_search_up() {
        let temp_dir = create_test_structure().await.unwrap();
        let base_path = temp_dir.path();

        // Create a subdirectory without node_modules
        let sub_dir = base_path.join("subdir").join("deeper");
        async_fs::create_dir_all(&sub_dir).await.unwrap();

        // Search from the subdirectory should find the binary in parent
        let result = find_binary_in_hierarchy(&sub_dir, "test-cmd")
            .await
            .unwrap();
        assert!(result.is_some());

        let found_path = result.unwrap();
        assert!(found_path.ends_with("node_modules/.bin/test-cmd"));
    }
}
