use crate::helper::compatibility::{is_cpu_compatible, is_os_compatible};
use crate::helper::package::parse_package_name;
use crate::model::package::{PackageInfo, Scripts};
use crate::util::json::{load_package_json_from_path, load_package_lock_json_from_path};
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_info, log_progress, log_verbose, start_progress_bar,
};
use anyhow::{Context, Result};
use futures;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::script::ScriptService;

pub struct PackageService;

impl PackageService {
    pub async fn process_project_hooks(root_path: &Path) -> Result<()> {
        let data = load_package_json_from_path(root_path)?;

        let binding = serde_json::Map::new();
        let scripts = data
            .get("scripts")
            .and_then(|s| s.as_object())
            .unwrap_or(&binding);

        let hooks = [
            "preinstall",
            "install",
            "postinstall",
            "prepublish",
            "preprepare",
            "prepare",
            "postprepare",
        ];

        let (scope, name, fullname) = parse_package_name(&format!(
            "node_modules/{}",
            data.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        ));

        let package_info = PackageInfo {
            path: root_path.to_path_buf(),
            bin_files: Vec::new(),
            scripts: Scripts {
                preinstall: scripts
                    .get("preinstall")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                install: scripts
                    .get("install")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                postinstall: scripts
                    .get("postinstall")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                prepare: scripts
                    .get("prepare")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                preprepare: scripts
                    .get("preprepare")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                postprepare: scripts
                    .get("postprepare")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                prepublish: scripts
                    .get("prepublish")
                    .and_then(|s| s.as_str())
                    .map(String::from),
            },
            name,
            fullname,
            version: data
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            scope,
        };

        for hook in hooks {
            if scripts.get(hook).and_then(|s| s.as_str()).is_some() {
                log_info(&format!("Executing project hook: {hook}"));
                ScriptService::execute_script(&package_info, hook, true)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to execute project hook {}: {}", hook, e)
                    })?;
            }
        }

        Ok(())
    }

    pub fn collect_packages(root_path: &Path) -> Result<Vec<PackageInfo>> {
        log_verbose("Collecting packages...");
        let lock_data = load_package_lock_json_from_path(root_path)?;

        let mut packages = Vec::new();
        if let Some(deps) = lock_data.get("packages").and_then(|v| v.as_object()) {
            for (path, info) in deps {
                if path.is_empty() {
                    continue;
                }
                if let Some(package) =
                    Self::process_package_info(&format!("{}/{}", root_path.display(), path), info)?
                {
                    packages.push(package);
                }
            }
        }
        Ok(packages)
    }

    pub fn create_execution_queues(packages: Vec<PackageInfo>) -> Result<Vec<Vec<PackageInfo>>> {
        log_verbose("Prepareing execute queues...");
        let mut queues = vec![Vec::new(); 5];

        // create queues, and we will check if there is a cache first
        // if there is a cache, we will not execute the scripts related tasks
        for package in packages {
            let has_cached = Self::has_cached(&package);
            if has_cached {
                log_verbose(&format!(
                    "Package {} is cached, skipping execution",
                    package.fullname
                ));
                queues[0].push(package.clone());
            }
            if package.scripts.preinstall.is_some() && !has_cached {
                log_verbose(&format!(
                    "Adding {} to preinstall queue",
                    package.path.display()
                ));
                queues[1].push(package.clone());
            }
            if !package.bin_files.is_empty() {
                log_verbose(&format!(
                    "Adding {} to bin linking queue",
                    package.path.display()
                ));
                queues[2].push(package.clone());
            }
            if package.scripts.install.is_some() && !has_cached {
                log_verbose(&format!(
                    "Adding {} to install queue",
                    package.path.display()
                ));
                queues[3].push(package.clone());
            }
            if package.scripts.postinstall.is_some() && !has_cached {
                log_verbose(&format!(
                    "Adding {} to postinstall queue",
                    package.path.display()
                ));
                queues[4].push(package.clone());
            }
        }

        log_verbose(&format!(
            "Queue creation completed, {} tasks pending",
            queues.iter().map(|q| q.len()).sum::<usize>()
        ));

        Ok(queues)
    }

    pub fn process_package_info(path: &str, info: &Value) -> Result<Option<PackageInfo>> {
        let info = match info.as_object() {
            Some(obj) => obj,
            None => return Ok(None),
        };

        // check if there is an install script or bin files
        let has_install_script = info
            .get("hasInstallScript")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let has_bin = info.get("bin").is_some();

        if !has_install_script && !has_bin {
            return Ok(None);
        }

        // check if the package is compatible with current platform
        let is_compatible = if let Some(cpu) = info.get("cpu") {
            is_cpu_compatible(cpu)
        } else {
            true
        } && if let Some(os) = info.get("os") {
            is_os_compatible(os)
        } else {
            true
        };

        if !is_compatible {
            log_verbose(&format!(
                "Package {path} is not compatible with current platform"
            ));
            return Ok(None);
        }

        // parse package name
        let (scope, name, fullname) = parse_package_name(path);

        // parse bin files
        let bin_files = Self::parse_bin_files(info.get("bin"), &name);

        // parse scripts
        let scripts = Self::read_package_scripts(Path::new(path))
            .context(format!("Failed to read scripts for package: {path}"))?;

        Ok(Some(PackageInfo {
            path: PathBuf::from(path),
            bin_files,
            scripts,
            name,
            fullname,
            version: info
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            scope,
        }))
    }

    fn parse_bin_files(bin: Option<&Value>, package_name: &str) -> Vec<(String, String)> {
        match bin {
            Some(Value::Object(obj)) => obj
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect(),
            Some(Value::String(s)) => vec![(package_name.to_string(), s.clone())],
            _ => Vec::new(),
        }
    }

    fn has_cached(_package: &PackageInfo) -> bool {
        // TODO: implement cache check
        false
    }

    fn read_package_scripts(package_path: &Path) -> Result<Scripts> {
        let data = load_package_json_from_path(package_path)?;

        let default_scripts = serde_json::Map::new();
        let scripts = data
            .get("scripts")
            .and_then(|s| s.as_object())
            .unwrap_or(&default_scripts);

        Ok(Scripts {
            preinstall: scripts
                .get("preinstall")
                .and_then(|s| s.as_str())
                .map(String::from),
            install: scripts
                .get("install")
                .and_then(|s| s.as_str())
                .map(String::from),
            postinstall: scripts
                .get("postinstall")
                .and_then(|s| s.as_str())
                .map(String::from),
            prepare: scripts
                .get("prepare")
                .and_then(|s| s.as_str())
                .map(String::from),
            preprepare: scripts
                .get("preprepare")
                .and_then(|s| s.as_str())
                .map(String::from),
            postprepare: scripts
                .get("postprepare")
                .and_then(|s| s.as_str())
                .map(String::from),
            prepublish: scripts
                .get("prepublish")
                .and_then(|s| s.as_str())
                .map(String::from),
        })
    }

    pub async fn execute_queues(queues: Vec<Vec<PackageInfo>>) -> Result<()> {
        let total_scripts = queues[1].len() + queues[3].len() + queues[4].len();
        if total_scripts > 0 {
            start_progress_bar();
            PROGRESS_BAR.set_length(total_scripts as u64);
        }

        // Execute preinstall scripts in parallel
        let preinstall_tasks: Vec<_> = queues[1]
            .iter()
            .filter_map(|package| {
                package.scripts.preinstall.as_ref().map(|script| {
                    let package = package.clone();
                    let script = script.clone();
                    async move {
                        log_progress(&format!("{} preinstall", package.fullname));
                        let result = ScriptService::execute_script(&package, "preinstall", false)
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to execute preinstall script for {} (command: {}): {}",
                                    package.fullname,
                                    script,
                                    e
                                )
                            });
                        PROGRESS_BAR.inc(1);
                        result
                    }
                })
            })
            .collect();

        // Wait for all preinstall tasks to complete
        let preinstall_results: Vec<Result<()>> = futures::future::join_all(preinstall_tasks).await;
        for result in preinstall_results {
            result?;
        }

        // Link binary files
        for package in &queues[2] {
            if !package.bin_files.is_empty() {
                log_verbose(&format!("Linking binary files for {}", package.fullname));
                for (bin_name, relative_path) in &package.bin_files {
                    let target_path = package.path.join(relative_path);
                    if !target_path.exists() {
                        log_verbose(&format!(
                            "Binary file {} does not exist, skipping",
                            target_path.display()
                        ));
                        continue;
                    }

                    let bin_dir = package.get_bin_dir().context(format!(
                        "Failed to get bin directory for {}",
                        package.fullname
                    ))?;
                    let link_path = bin_dir.join(bin_name);

                    ScriptService::ensure_executable(&target_path)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to ensure binary is executable for {} (path: {}): {}",
                                package.fullname,
                                target_path.display(),
                                e
                            )
                        })?;

                    crate::util::linker::link(&target_path, &link_path).context(format!(
                        "Failed to create symbolic link for {} (from: {} to: {})",
                        package.fullname,
                        target_path.display(),
                        link_path.display()
                    ))?;
                }
                log_verbose(&format!(
                    "Linking binary files for {} successfully",
                    package.fullname
                ));
            }
        }

        // Execute install scripts in parallel
        let install_tasks: Vec<_> = queues[3]
            .iter()
            .filter_map(|package| {
                package.scripts.install.as_ref().map(|script| {
                    let package = package.clone();
                    let script = script.clone();
                    async move {
                        log_progress(&format!("{} install", package.fullname));
                        let result = ScriptService::execute_script(&package, "install", false)
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to execute install script for {} (command: {}): {}",
                                    package.fullname,
                                    script,
                                    e
                                )
                            });
                        PROGRESS_BAR.inc(1);
                        result
                    }
                })
            })
            .collect();

        // Wait for all install tasks to complete
        let install_results: Vec<Result<()>> = futures::future::join_all(install_tasks).await;
        for result in install_results {
            result?;
        }

        // Execute postinstall scripts in parallel
        let postinstall_tasks: Vec<_> = queues[4]
            .iter()
            .filter_map(|package| {
                package.scripts.postinstall.as_ref().map(|script| {
                    let package = package.clone();
                    let script = script.clone();
                    async move {
                        log_progress(&format!("{} postinstall", package.fullname));
                        let result = ScriptService::execute_script(&package, "postinstall", false)
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to execute postinstall script for {} (command: {}): {}",
                                    package.fullname,
                                    script,
                                    e
                                )
                            });
                        PROGRESS_BAR.inc(1);
                        result
                    }
                })
            })
            .collect();

        // Wait for all postinstall tasks to complete
        let postinstall_results: Vec<Result<()>> =
            futures::future::join_all(postinstall_tasks).await;
        for result in postinstall_results {
            result?;
        }

        finish_progress_bar("scripts executed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_process_project_hooks_basic() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with basic project hooks
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "echo 'Running preinstall hook'",
                "postinstall": "echo 'Running postinstall hook'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test process_project_hooks
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_ok(), "process_project_hooks should succeed");
    }

    #[tokio::test]
    async fn test_process_project_hooks_no_scripts() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json without scripts
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0"
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test process_project_hooks - should succeed even without scripts
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "process_project_hooks should succeed even without scripts"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_with_scoped_package() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with scoped package name
        let package_json = json!({
            "name": "@scope/test-project",
            "version": "1.0.0",
            "scripts": {
                "prepare": "echo 'Running prepare hook for scoped package'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test process_project_hooks with scoped package
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "process_project_hooks should work with scoped packages"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_all_supported_hooks() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with all supported hooks
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "echo 'preinstall'",
                "install": "echo 'install'",
                "postinstall": "echo 'postinstall'",
                "prepublish": "echo 'prepublish'",
                "preprepare": "echo 'preprepare'",
                "prepare": "echo 'prepare'",
                "postprepare": "echo 'postprepare'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that all hooks are executed
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "All supported hooks should be executed successfully"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_working_directory() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create a subdirectory structure
        let sub_dir = project_path.join("subproject");
        fs::create_dir_all(&sub_dir).unwrap();

        // Create package.json in subdirectory with script that checks working directory
        let package_json = json!({
            "name": "test-subproject",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "pwd | grep subproject"
            }
        });

        fs::write(
            sub_dir.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that scripts run in the correct directory (root_path)
        let result = PackageService::process_project_hooks(&sub_dir).await;
        assert!(
            result.is_ok(),
            "Scripts should run in the correct working directory based on root_path"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_npm_package_json_env() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with script that checks npm_package_json environment variable
        let expected_package_json_path = project_path.join("package.json");
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": format!("test \"$npm_package_json\" = \"{}\"", expected_package_json_path.display())
            }
        });

        fs::write(
            &expected_package_json_path,
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that npm_package_json environment variable points to the correct path
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "npm_package_json environment variable should point to the correct package.json path"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_script_failure() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with failing script
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "exit 1"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that script failure is properly handled
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_err(),
            "Script failure should be properly propagated"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to execute project hook")
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_invalid_package_json() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create invalid package.json
        fs::write(project_path.join("package.json"), "invalid json content").unwrap();

        // Test that invalid package.json is properly handled
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_err(), "Invalid package.json should cause error");
    }

    #[tokio::test]
    async fn test_process_project_hooks_missing_package_json() {
        // Create temporary directory without package.json
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Test that missing package.json is properly handled
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_err(), "Missing package.json should cause error");
    }

    #[tokio::test]
    async fn test_process_project_hooks_partial_scripts() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with only some hooks
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "install": "echo 'install only'",
                "prepare": "echo 'prepare only'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that only existing hooks are executed
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_ok(), "Only existing hooks should be executed");
    }

    #[tokio::test]
    async fn test_process_project_hooks_different_root_paths() {
        // Create multiple project directories to test path isolation
        let temp_dir = TempDir::new().unwrap();
        let project1_path = temp_dir.path().join("project1");
        let project2_path = temp_dir.path().join("project2");

        fs::create_dir_all(&project1_path).unwrap();
        fs::create_dir_all(&project2_path).unwrap();

        // Create different package.json files
        let package_json1 = json!({
            "name": "project1",
            "version": "1.0.0",
            "scripts": {
                "preinstall": format!("test \"$npm_package_json\" = \"{}\"", project1_path.join("package.json").display())
            }
        });

        let package_json2 = json!({
            "name": "project2",
            "version": "2.0.0",
            "scripts": {
                "preinstall": format!("test \"$npm_package_json\" = \"{}\"", project2_path.join("package.json").display())
            }
        });

        fs::write(
            project1_path.join("package.json"),
            serde_json::to_string_pretty(&package_json1).unwrap(),
        )
        .unwrap();

        fs::write(
            project2_path.join("package.json"),
            serde_json::to_string_pretty(&package_json2).unwrap(),
        )
        .unwrap();

        // Test that each project gets the correct environment variables
        let result1 = PackageService::process_project_hooks(&project1_path).await;
        assert!(
            result1.is_ok(),
            "Project1 hooks should succeed with correct environment"
        );

        let result2 = PackageService::process_project_hooks(&project2_path).await;
        assert!(
            result2.is_ok(),
            "Project2 hooks should succeed with correct environment"
        );
    }

    #[tokio::test]
    async fn test_execute_queues_skips_missing_bin_file() {
        use std::fs;
        use tempfile::TempDir;

        // Create a temporary directory for the fake package
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path();

        // Create a package.json with a bin entry pointing to a non-existent file
        let package_json = serde_json::json!({
            "name": "test-bin-missing",
            "version": "1.0.0",
            "bin": {
                "testbin": "not-exist.js"
            }
        });
        fs::write(
            package_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Construct PackageInfo manually
        let package_info = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: vec![("testbin".to_string(), "not-exist.js".to_string())],
            scripts: Scripts {
                preinstall: None,
                install: None,
                postinstall: None,
                prepare: None,
                preprepare: None,
                postprepare: None,
                prepublish: None,
            },
            name: "test-bin-missing".to_string(),
            fullname: "test-bin-missing".to_string(),
            version: "1.0.0".to_string(),
            scope: None,
        };

        // Prepare queues: only bin linking queue (index 2) has this package
        let mut queues = vec![vec![], vec![], vec![], vec![], vec![]];
        queues[2].push(package_info);

        // Should not panic or error, even though the bin file does not exist
        let result = PackageService::execute_queues(queues).await;
        assert!(result.is_ok());
    }
}
