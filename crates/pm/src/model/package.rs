use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};

use crate::util::json::load_package_json_from_path;
use crate::{service::script::ScriptService, util::linker::link};

#[derive(Debug, Default, Clone)]
pub struct Scripts {
    pub preinstall: Option<String>,
    pub install: Option<String>,
    pub postinstall: Option<String>,
    pub prepare: Option<String>,
    pub preprepare: Option<String>,
    pub postprepare: Option<String>,
    pub prepublish: Option<String>,
}

impl Scripts {
    pub fn get_script(&self, script_type: &str) -> Option<&String> {
        match script_type {
            "preinstall" => self.preinstall.as_ref(),
            "install" => self.install.as_ref(),
            "postinstall" => self.postinstall.as_ref(),
            "prepare" => self.prepare.as_ref(),
            "preprepare" => self.preprepare.as_ref(),
            "postprepare" => self.postprepare.as_ref(),
            "prepublish" => self.prepublish.as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub path: PathBuf,
    pub bin_files: Vec<(String, String)>, // (bin_name, relative_path)
    pub scripts: Scripts,
    pub fullname: String, // exp "@babel/parser"
    pub name: String,     // exp "parser" or "@babel/parser"
}

impl PackageInfo {
    pub fn get_bin_dir(&self) -> Option<PathBuf> {
        match self
            .path
            .ancestors()
            .find(|p| p.ends_with("node_modules"))
            .map(|p| p.to_path_buf().join(".bin"))
        {
            Some(path) => Some(path),
            None => Some(PathBuf::from("node_modules/.bin")),
        }
    }

    pub fn has_bin_files(&self) -> bool {
        !self.bin_files.is_empty()
    }

    pub async fn from_path(path: &Path) -> Result<Self> {
        // Read package.json
        let data = load_package_json_from_path(path).await?;

        // Parse package name
        let name = data["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to get package name from package.json"))?
            .to_string();

        // Parse binary files
        let bin_files = if let Some(bin) = data.get("bin") {
            if bin.is_object() {
                bin.as_object()
                    .map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                            .collect()
                    })
                    .unwrap_or_default()
            } else if bin.is_string() {
                let bin_path = bin.as_str().unwrap_or_default().to_string();
                vec![(name.clone(), bin_path)]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse scripts
        let scripts = Scripts {
            preinstall: data["scripts"]["preinstall"].as_str().map(String::from),
            install: data["scripts"]["install"].as_str().map(String::from),
            postinstall: data["scripts"]["postinstall"].as_str().map(String::from),
            prepare: data["scripts"]["prepare"].as_str().map(String::from),
            preprepare: data["scripts"]["preprepare"].as_str().map(String::from),
            postprepare: data["scripts"]["postprepare"].as_str().map(String::from),
            prepublish: data["scripts"]["prepublish"].as_str().map(String::from),
        };

        Ok(PackageInfo {
            path: path.to_path_buf(),
            bin_files,
            scripts,
            name: name.clone(),
            fullname: name,
        })
    }

    pub async fn link_to_target(&self, target_bin_dir: &Path) -> Result<()> {
        // Link each binary file
        for (bin_name, relative_path) in &self.bin_files {
            let target_path = self.path.join(relative_path);
            let link_path = target_bin_dir.join(bin_name);

            tracing::debug!("Linking global binary: {bin_name} -> {relative_path}");

            // Ensure target file is executable
            ScriptService::ensure_executable(&target_path)
                .await
                .context("Failed to ensure binary is executable")?;

            // Create symbolic link
            link(&target_path, &link_path)
                .await
                .context("Failed to create symbolic link")?;
        }

        Ok(())
    }

    pub async fn link_to_global(&self, global_bin_dir: &Path) -> Result<()> {
        self.link_to_target(global_bin_dir).await?;

        // Update PATH environment variable for current process
        if let Ok(current_path) = env::var("PATH") {
            let global_bin_str = global_bin_dir.to_string_lossy().to_string();
            if !current_path.contains(&global_bin_str) {
                let new_path = format!("{global_bin_str}:{current_path}");
                unsafe { env::set_var("PATH", new_path) };
                tracing::debug!("Updated PATH environment variable");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_package_info_from_path() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir(&package_dir).unwrap();

        // Create a sample package.json
        let package_json = r#"
        {
            "name": "test-package",
            "version": "1.0.0",
            "bin": {
                "test-cli": "./bin/cli.js"
            },
            "scripts": {
                "preinstall": "echo preinstall",
                "install": "echo install",
                "postinstall": "echo postinstall"
            }
        }"#;
        fs::write(package_dir.join("package.json"), package_json).unwrap();

        // Create bin directory and file
        fs::create_dir(package_dir.join("bin")).unwrap();
        fs::write(
            package_dir.join("bin/cli.js"),
            "#!/usr/bin/env node\nconsole.log('test')",
        )
        .unwrap();

        // Test PackageInfo::from_path
        let package_info = PackageInfo::from_path(&package_dir).await.unwrap();

        assert_eq!(package_info.name, "test-package");
        assert_eq!(package_info.fullname, "test-package");
        assert_eq!(package_info.bin_files.len(), 1);
        assert_eq!(package_info.bin_files[0].0, "test-cli");
        assert_eq!(package_info.bin_files[0].1, "./bin/cli.js");
        assert!(package_info.scripts.preinstall.is_some());
        assert!(package_info.scripts.install.is_some());
        assert!(package_info.scripts.postinstall.is_some());
    }

    #[tokio::test]
    async fn test_package_info_from_path_with_scope() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("@scope/test-package");
        fs::create_dir_all(&package_dir).unwrap();

        // Create a sample package.json
        let package_json = r#"
        {
            "name": "@scope/test-package",
            "version": "1.0.0"
        }"#;
        fs::write(package_dir.join("package.json"), package_json).unwrap();

        // Test PackageInfo::from_path
        let package_info = PackageInfo::from_path(&package_dir).await.unwrap();

        assert_eq!(package_info.name, "@scope/test-package");
        assert_eq!(package_info.fullname, "@scope/test-package");
    }

    #[tokio::test]
    async fn test_package_info_from_path_invalid_json() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir(&package_dir).unwrap();

        // Create an invalid package.json
        fs::write(package_dir.join("package.json"), "invalid json").unwrap();

        // Test PackageInfo::from_path with invalid JSON
        let result = PackageInfo::from_path(&package_dir).await;
        assert!(result.is_err());
    }
}
