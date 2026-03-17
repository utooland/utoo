use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use std::env;
use std::path::{Path, PathBuf};
use utoo_ruborist::manifest::{PackageInstallView, PackageJson, PublishConfig};

use crate::util::json::load_package_json;
use crate::util::user_config::get_or_load_package_json;
use crate::{service::script::ScriptService, util::linker::link};

/// Typed struct for npm lifecycle hooks only.
/// For arbitrary user scripts (build, test, dev, etc.), use `PackageInfo.scripts` HashMap.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub struct LifecycleScripts {
    pub preinstall: Option<String>,
    pub install: Option<String>,
    pub postinstall: Option<String>,
    pub prepare: Option<String>,
    pub preprepare: Option<String>,
    pub postprepare: Option<String>,
    pub prepublish: Option<String>,
    #[serde(rename = "prepublishOnly")]
    pub prepublish_only: Option<String>,
    pub prepack: Option<String>,
    pub postpack: Option<String>,
    pub publish: Option<String>,
    pub postpublish: Option<String>,
}

impl LifecycleScripts {
    /// Roundtrip through serde to extract only the known lifecycle hook fields
    /// from a full scripts map, ignoring arbitrary user scripts (build, test, etc.).
    pub fn from_scripts(scripts: &HashMap<String, String>) -> Self {
        serde_json::to_value(scripts)
            .and_then(serde_json::from_value)
            .unwrap_or_default()
    }

    pub fn get_script(&self, script_type: &str) -> Option<&String> {
        match script_type {
            "preinstall" => self.preinstall.as_ref(),
            "install" => self.install.as_ref(),
            "postinstall" => self.postinstall.as_ref(),
            "prepare" => self.prepare.as_ref(),
            "preprepare" => self.preprepare.as_ref(),
            "postprepare" => self.postprepare.as_ref(),
            "prepublish" => self.prepublish.as_ref(),
            "prepublishOnly" => self.prepublish_only.as_ref(),
            "prepack" => self.prepack.as_ref(),
            "postpack" => self.postpack.as_ref(),
            "publish" => self.publish.as_ref(),
            "postpublish" => self.postpublish.as_ref(),
            _ => None,
        }
    }
}

/// Publish-related metadata extracted from package.json.
///
/// Combines the top-level `private` field with nested `publishConfig`.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub struct PublishMeta {
    pub private: bool,
    #[serde(default)]
    pub version: String,
    #[serde(rename = "publishConfig")]
    pub publish_config: PublishConfig,
}

impl PublishMeta {
    pub fn from_package_json(pkg: &PackageJson) -> Self {
        Self {
            private: pkg.private.unwrap_or(false),
            version: pkg.version.clone(),
            publish_config: pkg.publish_config.clone().unwrap_or_default(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.private {
            bail!(
                "This package has been marked as private.\n\
                 Remove the 'private' field from package.json to publish it."
            );
        }
        if self
            .publish_config
            .access
            .as_deref()
            .is_some_and(|a| a != "public")
        {
            bail!(
                "utoo publish currently only supports public access.\n\
                 Remove or change 'publishConfig.access' to \"public\" in package.json."
            );
        }
        Ok(())
    }

    /// Resolve the publish tag: CLI flag > publishConfig.tag > "latest".
    ///
    /// Rejects pre-release versions using the default `latest` tag to prevent
    /// accidentally marking a pre-release as the stable install target.
    pub fn resolve_tag(&self, cli_tag: Option<&str>) -> Result<String> {
        let is_default = cli_tag.is_none() && self.publish_config.tag.is_none();
        let tag = cli_tag
            .map(String::from)
            .or_else(|| self.publish_config.tag.clone())
            .unwrap_or_else(|| "latest".to_string());

        if is_default && self.version.contains('-') {
            bail!(
                "Publishing a pre-release version ({}) with the 'latest' tag is not allowed.\n\
                 Use --tag to specify an explicit tag, e.g.: utoo publish --tag beta",
                self.version,
            );
        }
        Ok(tag)
    }
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub path: PathBuf,
    pub bin_files: Vec<(String, String)>, // (bin_name, relative_path)
    pub scripts: HashMap<String, String>,
    pub lifecycle_scripts: LifecycleScripts,
    pub name: String, // Full scoped name, e.g. "@babel/parser"
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

    /// Load PackageInfo from disk without caching.
    /// For node_modules dependencies; project/workspace packages should use `load`.
    pub async fn from_path(path: &Path) -> Result<Self> {
        let pkg: PackageInstallView = load_package_json(path).await?;
        Self::from_install_view(path, &pkg)
    }

    /// Load PackageInfo using the cached package.json reader.
    /// Preferred for project/workspace packages.
    pub async fn load(path: &Path) -> Result<Self> {
        let pkg = get_or_load_package_json(path).await?;
        Self::from_package_json(path, &pkg)
    }

    /// Build from the full PackageJson (cached path, project/workspace packages).
    pub fn from_package_json(path: &Path, pkg: &PackageJson) -> Result<Self> {
        if pkg.name.is_empty() {
            anyhow::bail!("Failed to get package name from package.json");
        }
        Ok(PackageInfo {
            path: path.to_path_buf(),
            bin_files: pkg.bin_entries(),
            lifecycle_scripts: LifecycleScripts::from_scripts(pkg.scripts_or_empty()),
            scripts: pkg.scripts_or_empty().clone(),
            name: pkg.name.clone(),
        })
    }

    /// Build from the lightweight install view (node_modules packages).
    pub fn from_install_view(path: &Path, pkg: &PackageInstallView) -> Result<Self> {
        if pkg.name.is_empty() {
            anyhow::bail!("Failed to get package name from package.json");
        }
        Ok(PackageInfo {
            path: path.to_path_buf(),
            bin_files: pkg.bin_entries(),
            lifecycle_scripts: LifecycleScripts::from_scripts(&pkg.scripts),
            scripts: pkg.scripts.clone(),
            name: pkg.name.clone(),
        })
    }

    pub async fn link_to_target(&self, target_bin_dir: &Path) -> Result<()> {
        // Link each binary file
        for (bin_name, relative_path) in &self.bin_files {
            let target_path = self.path.join(relative_path);
            let link_path = target_bin_dir.join(bin_name);

            tracing::debug!("Linking binary: {bin_name} -> {relative_path}");

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
        assert_eq!(package_info.bin_files.len(), 1);
        assert_eq!(package_info.bin_files[0].0, "test-cli");
        assert_eq!(package_info.bin_files[0].1, "./bin/cli.js");
        assert!(package_info.lifecycle_scripts.preinstall.is_some());
        assert!(package_info.lifecycle_scripts.install.is_some());
        assert!(package_info.lifecycle_scripts.postinstall.is_some());
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
