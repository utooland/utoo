use crate::service::package::PackageService;
use crate::util::script_policy::{InstallScriptMode, ScriptPolicyArgs};
use anyhow::Result;
use std::path::Path;
use utoo_ruborist::lock::PackageLock;

pub struct RebuildService;

impl RebuildService {
    /// Core rebuild method - only processes memory objects.
    ///
    /// This is where every local install path (install / update / link /
    /// `utoo rebuild`) converges, so it owns resolving the install-time script
    /// policy from the CLI args + config + root `package.json`.
    ///
    /// # Arguments
    /// * `package_lock` - Package lock information in memory
    /// * `root_path` - Project root path (also the source of `allowScripts`)
    /// * `args` - CLI-level script policy flags
    pub async fn rebuild(
        package_lock: &PackageLock,
        root_path: &Path,
        args: &ScriptPolicyArgs,
    ) -> Result<()> {
        let mode = InstallScriptMode::resolve(args, Some(root_path)).await?;

        let packages =
            PackageService::collect_packages_from_lock(package_lock, root_path, &mode).await?;

        if !packages.is_empty() {
            let execution_queues =
                PackageService::create_execution_queues_with_options(packages, &mode)?;
            PackageService::execute_queues_with_options(execution_queues, &mode).await?;
        }

        // `--ignore-scripts` (IgnoreAll) also skips first-party project/workspace
        // hooks, as before. The dependency `allowScripts` policy does NOT gate
        // first-party hooks (they are operationally different), so policy and
        // allow-all modes both run them.
        if !mode.is_ignore_all() {
            // Run workspace `prepare`/`postinstall`/etc. in topological order
            // before the root project's hooks, since the root may import
            // build artifacts produced by a workspace's `prepare`.
            PackageService::process_workspace_install_hooks(root_path).await?;
            PackageService::process_project_hooks(root_path).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;
    use utoo_ruborist::lock::{LockPackage, PackageLock};
    use utoo_ruborist::manifest::BinField;

    fn create_test_package_lock() -> PackageLock {
        let mut packages = HashMap::new();

        // Package with both scripts and binaries
        packages.insert(
            "node_modules/test-package".to_string(),
            LockPackage {
                name: Some("test-package".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some(
                    "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string(),
                ),
                bin: Some(BinField::Map(std::collections::BTreeMap::from([(
                    "test-cli".to_string(),
                    "bin/cli.js".to_string(),
                )]))),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        // Package with only binaries
        packages.insert(
            "node_modules/bin-only-package".to_string(),
            LockPackage {
                name: Some("bin-only-package".to_string()),
                version: Some("2.0.0".to_string()),
                resolved: Some(
                    "https://registry.npmjs.org/bin-only-package/-/bin-only-package-2.0.0.tgz"
                        .to_string(),
                ),
                bin: Some(BinField::Map(std::collections::BTreeMap::from([(
                    "bin-tool".to_string(),
                    "index.js".to_string(),
                )]))),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        // Package with only scripts
        packages.insert("node_modules/script-only-package".to_string(), LockPackage {
            name: Some("script-only-package".to_string()),
            version: Some("3.0.0".to_string()),
            resolved: Some("https://registry.npmjs.org/script-only-package/-/script-only-package-3.0.0.tgz".to_string()),
            has_install_script: Some(true),
            ..LockPackage::default()
        });

        PackageLock::new("test-project".to_string(), "1.0.0".to_string(), packages)
    }

    fn create_test_project(temp_dir: &TempDir, packages: &[&str]) -> Result<()> {
        // Create package.json for root project
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "prepare": "echo 'prepare script'"
            }
        });
        fs::write(
            temp_dir.path().join("package.json"),
            serde_json::to_string_pretty(&package_json)?,
        )?;

        // Create node_modules and package directories
        let node_modules = temp_dir.path().join("node_modules");
        fs::create_dir_all(&node_modules)?;

        // Create .bin directory for binary linking
        let bin_dir = node_modules.join(".bin");
        fs::create_dir_all(&bin_dir)?;

        for package in packages {
            let package_dir = node_modules.join(package);
            fs::create_dir_all(&package_dir)?;

            let package_json = json!({
                "name": package,
                "version": "1.0.0",
                "scripts": {
                    "postinstall": "echo 'postinstall script'"
                },
                "bin": {
                    "test-cli": "bin/cli.js"
                }
            });
            fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_json)?,
            )?;

            // Create bin directory and file
            let bin_dir = package_dir.join("bin");
            fs::create_dir_all(&bin_dir)?;
            fs::write(
                bin_dir.join("cli.js"),
                "#!/usr/bin/env node\nconsole.log('test cli');",
            )?;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_rebuild_with_empty_package_lock() -> Result<()> {
        let temp_dir = TempDir::new()?;
        create_test_project(&temp_dir, &[])?;
        let package_lock = PackageLock::new(
            "test-project".to_string(),
            "1.0.0".to_string(),
            HashMap::new(),
        );

        // Test rebuild with empty package lock
        let result =
            RebuildService::rebuild(&package_lock, temp_dir.path(), &ScriptPolicyArgs::default())
                .await;
        assert!(
            result.is_ok(),
            "Rebuild with empty package lock should succeed"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_rebuild_missing_project_package_json() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let package_lock = create_test_package_lock();

        // Test rebuild without project package.json (should fail for project hooks)
        let result =
            RebuildService::rebuild(&package_lock, temp_dir.path(), &ScriptPolicyArgs::default())
                .await;
        assert!(
            result.is_err(),
            "Rebuild without project package.json should fail"
        );

        Ok(())
    }
}
