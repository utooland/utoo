use crate::helper::package::parse_package_name;
use crate::model::package::{PackageInfo, Scripts};
use crate::util::json::load_package_json_from_path;
use crate::util::logger::{PROGRESS_BAR, finish_progress_bar, log_progress, start_progress_bar};
use anyhow::{Context, Result};
use futures;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::model::package_json::parse_bin_field;

use super::script::ScriptService;

/// Execution queues for package scripts and binary linking
/// Each entry is (PackageInfo, is_optional) where is_optional indicates if the package
/// is an optional dependency (based on edge type in dependency graph)
#[derive(Default)]
pub struct ExecutionQueues {
    pub preinstall: Vec<(Rc<PackageInfo>, bool)>,
    pub bin_linking: Vec<(Rc<PackageInfo>, bool)>,
    pub install: Vec<(Rc<PackageInfo>, bool)>,
    pub postinstall: Vec<(Rc<PackageInfo>, bool)>,
}

pub struct PackageService;

impl PackageService {
    pub async fn process_project_hooks(root_path: &Path) -> Result<()> {
        let data = load_package_json_from_path(root_path).await?;

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

        let (_scope, name, fullname) = parse_package_name(&format!(
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
        };

        for hook in hooks {
            if scripts.get(hook).and_then(|s| s.as_str()).is_some() {
                tracing::debug!("Executing project hook: {hook}");
                ScriptService::execute_script(&package_info, hook, true)
                    .await
                    .with_context(|| format!("Failed to execute project hook {hook}"))?;
            }
        }

        Ok(())
    }

    async fn read_package_scripts(package_path: &Path) -> Result<Scripts> {
        let data = load_package_json_from_path(package_path).await?;

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

    /// Collect packages from memory PackageLock object with early filtering
    /// Returns Vec<(PackageInfo, is_optional)> where is_optional is determined by the edge type
    pub async fn collect_packages_from_lock(
        package_lock: &PackageLock,
        root_path: &Path,
        ignore_scripts: bool,
    ) -> Result<Vec<(PackageInfo, bool)>> {
        tracing::debug!("Collecting packages from memory lock...");

        let mut packages = Vec::new();
        for (path, lock_package) in &package_lock.packages {
            if path.is_empty() {
                continue;
            }

            // Early filtering based on ignore_scripts parameter
            let has_scripts = lock_package.has_install_scripts();
            let package_name = lock_package.get_name(path);
            let bin_files = lock_package
                .bin
                .as_ref()
                .map(|bin| parse_bin_field(bin, &package_name))
                .unwrap_or_default();
            let has_bin = !bin_files.is_empty();

            // Skip packages that don't meet the filter criteria
            if ignore_scripts && !has_bin {
                continue; // ignore_scripts mode: only process packages with binaries
            }
            if !ignore_scripts && !has_scripts && !has_bin {
                continue; // full mode: process packages with scripts or binaries
            }

            // Check platform compatibility
            let is_compatible = if let Some(cpu) = &lock_package.cpu {
                is_cpu_compatible(cpu)
            } else {
                true
            } && if let Some(os) = &lock_package.os {
                is_os_compatible(os)
            } else {
                true
            };

            if !is_compatible {
                tracing::debug!("Package {path} is not compatible with current platform");
                continue;
            }

            // Parse package name and create PackageInfo without reading package.json
            let (_scope, name, fullname) = parse_package_name(path);
            let package_path = PathBuf::from(format!("{}/{}", root_path.display(), path));

            // Skip if package directory doesn't exist (e.g., omitted by --production/--omit)
            if !package_path.exists() {
                tracing::debug!("Package {path} not installed, skipping rebuild");
                continue;
            }

            // Read scripts from package.json only if needed
            let scripts = if has_scripts || !ignore_scripts {
                Self::read_package_scripts(&package_path)
                    .await
                    .context(format!("Failed to read scripts for package: {path}"))?
            } else {
                // Create empty scripts for ignore_scripts mode
                Scripts {
                    preinstall: None,
                    install: None,
                    postinstall: None,
                    prepare: None,
                    preprepare: None,
                    postprepare: None,
                    prepublish: None,
                }
            };

            // Check if this package is an optional dependency (based on edge type)
            let is_optional =
                lock_package.optional == Some(true) || lock_package.dev_optional == Some(true);

            let package_info = PackageInfo {
                path: package_path,
                bin_files,
                scripts,
                name,
                fullname,
            };

            packages.push((package_info, is_optional));
        }
        Ok(packages)
    }

    /// Create execution queues with bins_only parameter support
    /// Takes Vec<(PackageInfo, is_optional)> where is_optional indicates edge type
    pub fn create_execution_queues_with_options(
        packages: Vec<(PackageInfo, bool)>,
        ignore_scripts: bool,
    ) -> Result<ExecutionQueues> {
        tracing::debug!("Creating execution queues with options...");
        let mut queues = ExecutionQueues::default();

        for (package, is_optional) in packages {
            let package = Rc::new(package);

            // Script queues - skip in bins_only mode
            if !ignore_scripts {
                if package.scripts.preinstall.is_some() {
                    tracing::debug!("Adding {} to preinstall queue", package.path.display());
                    queues.preinstall.push((Rc::clone(&package), is_optional));
                }
                if package.scripts.install.is_some() {
                    tracing::debug!("Adding {} to install queue", package.path.display());
                    queues.install.push((Rc::clone(&package), is_optional));
                }
                if package.scripts.postinstall.is_some() {
                    tracing::debug!("Adding {} to postinstall queue", package.path.display());
                    queues.postinstall.push((Rc::clone(&package), is_optional));
                }
            }

            // Binary linking queue - always process if package has bin files
            if !package.bin_files.is_empty() {
                tracing::debug!("Adding {} to bin linking queue", package.path.display());
                queues.bin_linking.push((Rc::clone(&package), is_optional));
            }
        }

        tracing::debug!(
            "Queue creation completed, {} tasks pending",
            queues.preinstall.len()
                + queues.bin_linking.len()
                + queues.install.len()
                + queues.postinstall.len()
        );

        Ok(queues)
    }

    /// Execute queues with bins_only parameter support
    pub async fn execute_queues_with_options(
        queues: ExecutionQueues,
        ignore_scripts: bool,
    ) -> Result<()> {
        if ignore_scripts {
            // Binary-only mode: only execute binary linking
            Self::execute_binary_linking(&queues.bin_linking).await?;
        } else {
            // Full mode: execute all queues in sequence
            let total_scripts =
                queues.preinstall.len() + queues.install.len() + queues.postinstall.len();
            if total_scripts > 0 {
                start_progress_bar();
                PROGRESS_BAR.set_length(total_scripts as u64);
            }

            // Execute preinstall scripts in parallel
            Self::execute_script_queue(&queues.preinstall, "preinstall").await?;

            // Link binary files
            Self::execute_binary_linking(&queues.bin_linking).await?;

            // Execute install scripts in parallel
            Self::execute_script_queue(&queues.install, "install").await?;

            // Execute postinstall scripts in parallel
            Self::execute_script_queue(&queues.postinstall, "postinstall").await?;

            if total_scripts > 0 {
                finish_progress_bar("scripts executed");
            }
        }
        Ok(())
    }

    /// Execute script queue for a specific script type
    /// Queue contains (PackageInfo, is_optional) tuples where is_optional indicates edge type
    async fn execute_script_queue(
        queue: &[(Rc<PackageInfo>, bool)],
        script_name: &str,
    ) -> Result<()> {
        use futures;

        let queue_start = std::time::Instant::now();
        tracing::debug!(
            "Starting {} queue with {} scripts",
            script_name,
            queue.len()
        );

        let script_tasks: Vec<_> = queue
            .iter()
            .filter_map(|(package, is_optional)| {
                let script_option = match script_name {
                    "preinstall" => &package.scripts.preinstall,
                    "install" => &package.scripts.install,
                    "postinstall" => &package.scripts.postinstall,
                    _ => return None,
                };

                script_option.as_ref().map(|script| {
                    let package = Rc::clone(package);
                    let script = script.clone();
                    let is_optional = *is_optional;
                    async move {
                        log_progress(&format!("{} {}", package.fullname, script_name));
                        let start = std::time::Instant::now();
                        let result = ScriptService::execute_script(&package, script_name, false)
                            .await
                            .with_context(|| {
                                format!(
                                    "Failed to execute {} script for {} (command: {})",
                                    script_name, package.fullname, script
                                )
                            });
                        let elapsed = start.elapsed();
                        tracing::debug!(
                            "[{:.2}s] {} {} completed (path: {}, script: {})",
                            elapsed.as_secs_f64(),
                            package.fullname,
                            script_name,
                            package.path.display(),
                            script
                        );
                        PROGRESS_BAR.inc(1);
                        (is_optional, result)
                    }
                })
            })
            .collect();

        // Wait for all script tasks to complete
        let script_results: Vec<(bool, Result<()>)> = futures::future::join_all(script_tasks).await;
        for (is_optional, result) in script_results {
            if let Err(e) = result {
                if is_optional {
                    tracing::warn!("Optional dependency script failed (ignored): {e}");
                } else {
                    return Err(e);
                }
            }
        }

        let queue_elapsed = queue_start.elapsed();
        tracing::debug!(
            "{} queue completed in {:.2}s",
            script_name,
            queue_elapsed.as_secs_f64()
        );

        Ok(())
    }

    /// Execute binary file linking for packages
    /// Queue contains (PackageInfo, is_optional) tuples - is_optional is not used here
    /// as binary linking happens only for successfully installed packages
    async fn execute_binary_linking(queue: &[(Rc<PackageInfo>, bool)]) -> Result<()> {
        for (package, _is_optional) in queue {
            if !package.bin_files.is_empty() {
                tracing::debug!("Linking binary files for {}", package.fullname);
                for (bin_name, relative_path) in &package.bin_files {
                    let target_path = package.path.join(relative_path);
                    if !crate::fs::try_exists(&target_path).await? {
                        tracing::debug!(
                            "Binary file {} does not exist, skipping",
                            target_path.display()
                        );
                        continue;
                    }

                    let bin_dir = package.get_bin_dir().context(format!(
                        "Failed to get bin directory for {}",
                        package.fullname
                    ))?;
                    let link_path = bin_dir.join(bin_name);

                    ScriptService::ensure_executable(&target_path)
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to ensure binary is executable for {} (path: {})",
                                package.fullname,
                                target_path.display()
                            )
                        })?;

                    crate::util::linker::link(&target_path, &link_path)
                        .await
                        .context(format!(
                            "Failed to create symbolic link for {} (from: {} to: {})",
                            package.fullname,
                            target_path.display(),
                            link_path.display()
                        ))?;
                }
                tracing::debug!("Linking binary files for {} successfully", package.fullname);
            }
        }
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
        };

        // Prepare queues: only bin linking queue has this package
        // The bool indicates is_optional (false = not optional)
        let queues = ExecutionQueues {
            bin_linking: vec![(Rc::new(package_info), false)],
            ..Default::default()
        };

        // Should not panic or error, even though the bin file does not exist
        let result = PackageService::execute_queues_with_options(queues, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_collect_packages_from_lock_with_ignore_scripts() {
        use serde_json::json;
        use std::collections::HashMap;
        use tempfile::TempDir;
        use utoo_ruborist::lock::{LockPackage, PackageLock};

        let temp_dir = TempDir::new().unwrap();

        // Create test packages in memory
        let mut packages = HashMap::new();

        // Package with both scripts and binaries
        packages.insert(
            "node_modules/full-package".to_string(),
            LockPackage {
                name: Some("full-package".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"cli": "bin/cli.js"})),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        // Package with only binaries
        packages.insert(
            "node_modules/bin-only".to_string(),
            LockPackage {
                name: Some("bin-only".to_string()),
                version: Some("2.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "index.js"})),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        // Package with only scripts
        packages.insert(
            "node_modules/script-only".to_string(),
            LockPackage {
                name: Some("script-only".to_string()),
                version: Some("3.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        // Package with neither scripts nor binaries
        packages.insert(
            "node_modules/no-hooks".to_string(),
            LockPackage {
                name: Some("no-hooks".to_string()),
                version: Some("4.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        let package_lock =
            PackageLock::new("test-project".to_string(), "1.0.0".to_string(), packages);

        // Create minimal package.json files for testing
        let node_modules = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&node_modules).unwrap();

        for package_name in &["full-package", "bin-only", "script-only", "no-hooks"] {
            let package_dir = node_modules.join(package_name);
            std::fs::create_dir_all(&package_dir).unwrap();

            let package_json = json!({
                "name": package_name,
                "version": "1.0.0",
                "scripts": {
                    "postinstall": "echo postinstall"
                }
            });
            std::fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_json).unwrap(),
            )
            .unwrap();
        }

        // Test ignore_scripts = false (should collect packages with scripts or binaries)
        let result =
            PackageService::collect_packages_from_lock(&package_lock, temp_dir.path(), false).await;
        assert!(result.is_ok());
        let packages_full = result.unwrap();
        assert_eq!(packages_full.len(), 3); // full-package, bin-only, script-only (no-hooks excluded)

        // Test ignore_scripts = true (should only collect packages with binaries)
        let result =
            PackageService::collect_packages_from_lock(&package_lock, temp_dir.path(), true).await;
        assert!(result.is_ok());
        let packages_bins_only = result.unwrap();
        assert_eq!(packages_bins_only.len(), 2); // full-package, bin-only (script-only and no-hooks excluded)

        // Verify the collected packages have correct bin_files
        for (package_info, _is_optional) in &packages_bins_only {
            assert!(
                !package_info.bin_files.is_empty(),
                "Package {} should have bin_files in ignore_scripts mode",
                package_info.fullname
            );
        }
    }

    #[tokio::test]
    async fn test_collect_packages_from_lock_platform_compatibility() {
        use serde_json::json;
        use std::collections::HashMap;
        use tempfile::TempDir;
        use utoo_ruborist::lock::{LockPackage, PackageLock};

        let temp_dir = TempDir::new().unwrap();

        let mut packages = HashMap::new();

        // Package with incompatible OS
        packages.insert(
            "node_modules/win-only".to_string(),
            LockPackage {
                name: Some("win-only".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "tool.exe"})),
                has_install_script: Some(false),
                os: Some(json!(["win32"])), // Only Windows
                ..LockPackage::default()
            },
        );

        // Package with compatible platform
        packages.insert(
            "node_modules/cross-platform".to_string(),
            LockPackage {
                name: Some("cross-platform".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "tool.js"})),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        let package_lock =
            PackageLock::new("test-project".to_string(), "1.0.0".to_string(), packages);

        // Create minimal package.json files
        let node_modules = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&node_modules).unwrap();

        {
            let package_name = &"cross-platform";
            // Only create compatible package
            let package_dir = node_modules.join(package_name);
            std::fs::create_dir_all(&package_dir).unwrap();

            let package_json = json!({
                "name": package_name,
                "version": "1.0.0"
            });
            std::fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_json).unwrap(),
            )
            .unwrap();
        }

        // Test that only compatible packages are collected
        let result =
            PackageService::collect_packages_from_lock(&package_lock, temp_dir.path(), true).await;
        assert!(result.is_ok());
        let packages_collected = result.unwrap();

        // Should only collect the cross-platform package (win-only filtered out by platform check)
        assert_eq!(packages_collected.len(), 1);
        assert_eq!(packages_collected[0].0.fullname, "cross-platform");
    }
}
