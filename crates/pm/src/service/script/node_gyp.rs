//! node-gyp bootstrap for packages with native addon builds.

use std::env;
use std::path::Path;
use tokio::process::Command;

use anyhow::Result;
use tokio::sync::OnceCell;

use super::ScriptService;
use crate::model::package::PackageInfo;
use crate::util::platform_const::PATH_SEPARATOR;

/// Only successful preparation is cached; failures and cancellation may retry.
static NODE_GYP_ENSURED: OnceCell<bool> = OnceCell::const_new();

fn bootstrap_command() -> Result<Command> {
    let mut command = Command::new(env::current_exe()?);
    command
        .args(["i", "-g", "node-gyp"])
        .env(crate::helper::auto_update::INTERNAL_UPDATE_ENV, "1")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    Ok(command)
}

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
            .get_or_try_init(|| async {
                if Self::has_node_gyp_in_path() {
                    tracing::debug!("node-gyp found in PATH");
                    return Ok(true);
                }

                tracing::debug!("node-gyp not found in PATH, installing globally");
                let status = bootstrap_command()?.status().await?;
                anyhow::ensure!(
                    status.success(),
                    "Failed to install node-gyp globally: {status}"
                );
                tracing::debug!("node-gyp installed successfully");
                Ok::<_, anyhow::Error>(true)
            })
            .await;

        result.copied()
    }

    pub fn is_node_gyp_pkg(package: &PackageInfo) -> bool {
        // https://hitu.antgroup-inc.cn/packages/@npmcli/node-gyp/files/lib/index.js#L6:L6
        package.path.join("binding.gyp").exists()
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

    #[test]
    fn bootstrap_uses_current_executable() {
        let command = bootstrap_command().unwrap();
        assert_eq!(command.as_std().get_program(), env::current_exe().unwrap());
        assert_eq!(
            command.as_std().get_args().collect::<Vec<_>>(),
            ["i", "-g", "node-gyp"]
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
