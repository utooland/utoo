use crate::service::package_management::PackageManagementService;
use crate::util::binary_resolver;
use anyhow::{Result, anyhow};
use std::path::Path;
use std::process::{Command, Stdio};

/// Execute a package binary
pub async fn execute_package(command: &str, args: Vec<String>) -> Result<()> {
    tracing::debug!("Executing command: {command} with args: {args:?}");

    // First, try to find the binary in local node_modules/.bin directories
    if let Some(binary_path) = binary_resolver::find_binary(command).await? {
        tracing::debug!("Found binary at: {}", binary_path.display());
        return execute_binary(&binary_path, args).await;
    }

    // If not found locally, try to install the package to cache
    tracing::debug!("Command '{command}' not found locally");

    // For now, assume the package name is the same as the command
    // This can be enhanced later to handle more complex package/command mappings
    // command is must be a valid package name
    let package_name = command;

    // Install the package to cache
    let package_cache_dir =
        PackageManagementService::install_package_to_cache(package_name).await?;

    // Try to find the binary in the cached package
    // utoo -x eslint --version
    // utoo -x @modelcontextprotocol/create-server --version
    // utoo -x @modelcontextprotocol/create-server create-mcp-server --version
    match binary_resolver::find_binary_in_cache(&package_cache_dir).await {
        Ok(Some(binary_path)) => {
            tracing::debug!("Found binary in cache at: {}", binary_path.display());
            execute_binary(&binary_path, args).await
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
        Err(anyhow!("Command failed with exit code: {exit_code}"))
    }
}
