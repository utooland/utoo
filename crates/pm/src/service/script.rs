use crate::model::package::PackageInfo;
use crate::util::logger::log_verbose;
use anyhow::{Context, Result};
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;

use super::binary::get_envs;

pub struct ScriptService;

impl ScriptService {
    /// Check if node-gyp exists in PATH by searching directories
    fn has_node_gyp_in_path() -> bool {
        if let Ok(paths) = env::var("PATH") {
            paths
                .split(':')
                .map(|dir| Path::new(dir).join("node-gyp"))
                .any(|path| std::path::Path::new(&path).exists())
        } else {
            false
        }
    }

    /// Ensure node-gyp is available in PATH, install globally if not
    pub async fn ensure_node_gyp() -> Result<bool> {
        // Check if node-gyp exists in PATH
        let has_node_gyp = Self::has_node_gyp_in_path();

        if !has_node_gyp {
            log_verbose("node-gyp not found in PATH, installing globally");
            // Install node-gyp globally, ignore stdout but keep stderr
            let status = Command::new("ut")
                .args(["i", "-g", "node-gyp"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .context("Failed to install node-gyp globally")?;
            if !status.success() {
                log_verbose(&format!("Failed to install node-gyp globally: {status}"));
                anyhow::bail!("Failed to install node-gyp globally");
            }
            return Ok(true);
        }
        Ok(true)
    }

    pub fn is_node_gyp_pkg(package: &PackageInfo) -> bool {
        // https://hitu.antgroup-inc.cn/packages/@npmcli/node-gyp/files/lib/index.js#L6:L6
        std::path::Path::new(&package.path.join("binding.gyp")).exists()
    }

    pub async fn execute_script(
        package: &PackageInfo,
        script_type: &str,
        show_output: bool,
    ) -> Result<()> {
        let script = package.scripts.get_script(script_type);

        if let Some(script) = script {
            log_verbose(&format!(
                "Executing {} script for {}: {}",
                script_type,
                package.path.display(),
                script
            ));

            if Self::is_node_gyp_pkg(package) {
                Self::ensure_node_gyp().await?;
            }

            let bin_paths = Self::collect_bin_paths(package).await?;
            let env_path = Self::build_path_env(&bin_paths);

            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(script)
                .current_dir(&package.path)
                .env("PATH", env_path)
                .env("npm_lifecycle_event", script_type)
                .env(
                    "INIT_CWD",
                    env::current_dir()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                )
                .env(
                    "npm_package_json",
                    package.path.join("package.json").display().to_string(),
                )
                .env("npm_config_prefix", "")
                .env("npm_config_global", "false");

            if let Some(envs) = get_envs().await {
                for (key, value) in envs {
                    if let Some(value_str) = value.as_str() {
                        cmd.env(key, value_str);
                    }
                }
            }
            log_verbose(&format!("Executing command: {cmd:?}"));

            let output = tokio::process::Command::from(cmd)
                .output()
                .await
                .context("Failed to execute script")?;

            if show_output && !output.stdout.is_empty() {
                println!("{}", String::from_utf8_lossy(&output.stdout));
            }

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let exit_code = output.status.code().unwrap_or(-1);
                anyhow::bail!(
                    "Script execution failed for {} in {}:\nCommand: {}\nExit code: {}\nStderr: {}\nStdout: {}",
                    script_type,
                    package.path.display(),
                    script,
                    exit_code,
                    stderr,
                    stdout
                );
            }
        }

        Ok(())
    }

    pub async fn ensure_executable(target_path: &Path) -> Result<()> {
        let permissions = tokio::fs::metadata(&target_path)
            .await
            .context(format!(
                "Failed to get file permissions {}",
                target_path.display()
            ))?
            .permissions();

        let is_executable = permissions.mode() & 0o111 != 0;

        if !is_executable {
            let mut content = fs::read_to_string(&target_path)
                .await
                .context(format!("Failed to read file {}", target_path.display()))?;

            if !content.starts_with("#!") {
                content = format!("#!/usr/bin/env node\n{content}");
                fs::write(&target_path, content)
                    .await
                    .context(format!("Failed to write shebang {}", target_path.display()))?;
            }
        }

        let mut perms = permissions;
        perms.set_mode(0o755);
        fs::set_permissions(&target_path, perms)
            .await
            .context("Failed to set executable permissions")?;

        Ok(())
    }

    async fn collect_bin_paths(package: &PackageInfo) -> Result<Vec<PathBuf>> {
        let mut bin_paths = Vec::new();
        let mut current_path = Some(package.path.as_path());

        while let Some(path) = current_path {
            let bin_path = path.join("node_modules/.bin");
            if tokio::fs::try_exists(&bin_path).await?
                && let Ok(absolute_path) = tokio::fs::canonicalize(&bin_path).await
            {
                bin_paths.push(absolute_path);
            }
            current_path = path.parent();
        }

        Ok(bin_paths)
    }

    fn build_path_env(bin_paths: &[PathBuf]) -> String {
        let path_separator = ":";
        let original_path = env::var("PATH").unwrap_or_default();
        let additional_paths = bin_paths
            .iter()
            .map(|p| p.display().to_string())
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
    ) -> Result<()> {
        Self::execute_custom_script_with_args(package, script_name, script_content, vec![]).await
    }

    pub async fn execute_custom_script_with_args(
        package: &PackageInfo,
        script_name: &str,
        script_content: &str,
        script_args: Vec<&str>,
    ) -> Result<()> {
        log_verbose(&format!(
            "Executing custom script for {}: {}",
            package.path.display(),
            script_name
        ));

        let bin_paths = Self::collect_bin_paths(package).await?;
        let env_path = Self::build_path_env(&bin_paths);

        let cmd_content = match script_args.is_empty() {
            true => script_content,
            false => &format!("{} {}", script_content, script_args.join(" ")),
        };

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(cmd_content)
            .current_dir(&package.path)
            .env("PATH", env_path)
            .env("npm_lifecycle_event", script_name)
            .env(
                "INIT_CWD",
                env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )
            .env(
                "npm_package_json",
                package.path.join("package.json").display().to_string(),
            )
            .env("npm_config_prefix", "")
            .env("npm_config_global", "false")
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        if let Some(envs) = get_envs().await {
            for (key, value) in envs {
                if let Some(value_str) = value.as_str() {
                    cmd.env(key, value_str);
                }
            }
        }

        let status = cmd.status().context("Failed to execute custom script")?;

        if !status.success() {
            anyhow::bail!(
                "Custom script execution failed with exit code: {}",
                status.code().unwrap_or(-1)
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::model::package::Scripts;

    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_execute_custom_script_success() {
        let temp_dir = tempdir().unwrap();
        let package = PackageInfo {
            path: temp_dir.path().to_path_buf(),
            bin_files: Default::default(),
            scripts: Scripts::default(),
            scope: None,
            fullname: "test-package".to_string(),
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
        };

        let result =
            ScriptService::execute_custom_script(&package, "test", "echo 'test script'").await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_custom_script_failure() {
        let temp_dir = tempdir().unwrap();
        let package = PackageInfo {
            path: temp_dir.path().to_path_buf(),
            bin_files: Default::default(),
            scripts: Scripts::default(),
            scope: None,
            fullname: "test-package".to_string(),
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
        };

        let result = ScriptService::execute_custom_script(&package, "test", "exit 1").await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Custom script execution failed")
        );
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
        fs::set_permissions(&dummy_bin, fs::Permissions::from_mode(0o755)).unwrap();

        let package = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: Default::default(),
            scripts: Scripts::default(),
            scope: None,
            fullname: "test-package".to_string(),
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
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
            use std::os::unix::fs::PermissionsExt;
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

    #[test]
    fn test_has_node_gyp_in_path_found() {
        use std::env;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::tempdir;

        // Create a temporary directory
        let temp_dir = tempdir().unwrap();
        let node_gyp_path = temp_dir.path().join("node-gyp");
        // Create a dummy node-gyp executable
        fs::write(&node_gyp_path, "#!/bin/sh\necho node-gyp").unwrap();
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

    #[test]
    fn test_has_node_gyp_in_path_not_found() {
        use std::env;
        // Save original PATH
        let original_path = env::var("PATH").unwrap_or_default();
        // Set PATH to a temp dir without node-gyp
        let temp_dir = tempfile::tempdir().unwrap();
        unsafe {
            env::set_var("PATH", temp_dir.path());
        }
        // Should not find node-gyp
        assert!(!ScriptService::has_node_gyp_in_path());
        // Restore original PATH
        unsafe {
            env::set_var("PATH", original_path);
        }
    }

    #[tokio::test]
    async fn test_ensure_node_gyp_found() {
        use std::env;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let node_gyp_path = temp_dir.path().join("node-gyp");
        // Create a dummy node-gyp executable
        fs::write(&node_gyp_path, "#!/bin/sh\necho node-gyp").unwrap();
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
        use std::fs;
        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let package_path = temp_dir.path();
        // Case 1: binding.gyp exists
        let binding_gyp = package_path.join("binding.gyp");
        fs::write(&binding_gyp, "{}").unwrap();
        let package = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: Default::default(),
            scripts: Scripts::default(),
            scope: None,
            fullname: "test-package".to_string(),
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
        };
        assert!(ScriptService::is_node_gyp_pkg(&package));
        // Case 2: binding.gyp does not exist
        fs::remove_file(&binding_gyp).unwrap();
        assert!(!ScriptService::is_node_gyp_pkg(&package));
    }
}
