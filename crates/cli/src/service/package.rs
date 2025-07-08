use crate::helper::compatibility::{is_cpu_compatible, is_os_compatible};
use crate::helper::package::parse_package_name;
use crate::model::package::{PackageInfo, Scripts};
use crate::util::json::{load_package_json_from_path, load_package_lock_json_from_path};
use crate::util::logger::{
    finish_progress_bar, log_info, log_progress, log_verbose, start_progress_bar, PROGRESS_BAR,
};
use anyhow::{Context, Result};
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
            path: PathBuf::from("."),
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
        start_progress_bar();
        let total_scripts = queues[1].len() + queues[3].len() + queues[4].len();
        PROGRESS_BAR.set_length(total_scripts as u64);
        // Execute preinstall scripts
        for package in &queues[1] {
            if let Some(script) = &package.scripts.preinstall {
                log_progress(&format!(
                    "Executing preinstall script for {}",
                    package.fullname
                ));
                ScriptService::execute_script(package, "preinstall", false)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to execute preinstall script for {} (command: {}): {}",
                            package.fullname,
                            script,
                            e
                        )
                    })?;
                PROGRESS_BAR.inc(1);
            }
        }

        // Link binary files
        for package in &queues[2] {
            if !package.bin_files.is_empty() {
                log_verbose(&format!("Linking binary files for {}", package.fullname));
                for (bin_name, relative_path) in &package.bin_files {
                    let target_path = package.path.join(relative_path);
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

        // Execute install scripts
        for package in &queues[3] {
            if let Some(script) = &package.scripts.install {
                log_progress(&format!(
                    "Executing install script for {}",
                    package.fullname
                ));
                ScriptService::execute_script(package, "install", false)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to execute install script for {} (command: {}): {}",
                            package.fullname,
                            script,
                            e
                        )
                    })?;
                PROGRESS_BAR.inc(1);
            }
        }

        // Execute postinstall scripts
        for package in &queues[4] {
            if let Some(script) = &package.scripts.postinstall {
                log_progress(&format!(
                    "Executing postinstall script for {}",
                    package.fullname
                ));
                ScriptService::execute_script(package, "postinstall", false)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to execute postinstall script for {} (command: {}): {}",
                            package.fullname,
                            script,
                            e
                        )
                    })?;
                PROGRESS_BAR.inc(1);
            }
        }

        finish_progress_bar("Executing dependency hook scripts successfully");
        Ok(())
    }
}
