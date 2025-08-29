use crate::util::binary_resolver;
use crate::util::logger::{log_error, log_info, log_verbose};
use crate::util::package_installer;
use anyhow::{Result, anyhow};
use std::path::Path;
use std::process::{Command, Stdio};

/// Execute a package binary
pub async fn execute_package(command: &str, args: Vec<String>) -> Result<()> {
    log_verbose(&format!("Executing command: {command} with args: {args:?}"));

    // First, try to find the binary in local node_modules/.bin directories
    if let Some(binary_path) = binary_resolver::find_binary(command).await? {
        log_verbose(&format!("Found binary at: {}", binary_path.display()));
        return execute_binary(&binary_path, args).await;
    }

    // If not found locally, try to install the package to cache
    log_verbose(&format!("Command '{command}' not found locally"));

    // For now, assume the package name is the same as the command
    // This can be enhanced later to handle more complex package/command mappings
    // command is must be a valid package name
    let package_name = command;

    // Install the package to cache
    let package_cache_dir = package_installer::install_package_to_cache(package_name).await?;

    // Try to find the binary in the cached package
    // utoo -x eslint --version
    // utoo -x @modelcontextprotocol/create-server --version
    // utoo -x @modelcontextprotocol/create-server create-mcp-server --version
    match binary_resolver::find_binary_in_cache(&package_cache_dir) {
        Ok(Some(binary_path)) => {
            log_verbose(&format!(
                "Found binary in cache at: {}",
                binary_path.display()
            ));
            execute_binary(&binary_path, args).await
        }
        Ok(None) => {
            log_error(&format!(
                "No executable found in bin directory for package '{package_name}'"
            ));
            log_info(
                "The package might not provide any executables, or the bin directory might be empty",
            );
            Err(anyhow!(
                "No executable found for package '{}'",
                package_name
            ))
        }
        Err(e) => {
            log_error(&format!(
                "Error finding binary for package '{package_name}': {e}"
            ));
            Err(e)
        }
    }
}

/// Execute the binary with given arguments
async fn execute_binary(binary_path: &Path, args: Vec<String>) -> Result<()> {
    let mut cmd = Command::new(binary_path);
    cmd.args(&args);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd.status()?;

    if status.success() {
        Ok(())
    } else {
        let exit_code = status.code().unwrap_or(-1);
        Err(anyhow!("Command failed with exit code: {}", exit_code))
    }
}
