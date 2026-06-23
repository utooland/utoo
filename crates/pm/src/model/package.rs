use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use std::env;
use std::path::{Path, PathBuf};
use utoo_ruborist::manifest::{PackageInstallView, PackageJson, PublishConfig};

use crate::util::json::load_package_json;
use crate::util::platform_const::PATH_SEPARATOR;
use crate::util::user_config::get_or_load_package_json;
use crate::{service::script::ScriptService, util::linker::link};

/// Known npm lifecycle hook names.
///
/// Single source of truth — strum derives `Display`, `EnumString`, `IntoStaticStr`,
/// and `EnumIter` so hook names are defined once via `serialize_all` + per-variant overrides.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString, strum::IntoStaticStr,
)]
#[strum(serialize_all = "lowercase")]
pub enum LifecycleHook {
    Preinstall,
    Install,
    Postinstall,
    Prepare,
    Preprepare,
    Postprepare,
    Prepublish,
    #[strum(serialize = "prepublishOnly")]
    PrepublishOnly,
    Prepack,
    Postpack,
    Publish,
    Postpublish,
}

/// Lifecycle scripts extracted from package.json.
/// Only contains known npm lifecycle hooks; arbitrary user scripts are in `PackageInfo.scripts`.
#[derive(Debug, Default, Clone)]
pub struct LifecycleScripts {
    scripts: HashMap<LifecycleHook, String>,
}

impl LifecycleScripts {
    /// Extract lifecycle hooks from a scripts map, filtering out non-lifecycle entries.
    pub fn from_scripts(scripts: &HashMap<String, String>) -> Self {
        Self {
            scripts: scripts
                .iter()
                .filter_map(|(k, v)| Some((k.parse::<LifecycleHook>().ok()?, v.clone())))
                .collect(),
        }
    }

    pub fn get_script(&self, hook: LifecycleHook) -> Option<&str> {
        self.scripts.get(&hook).map(|s| s.as_str())
    }

    /// Whether any install-phase hook (`preinstall`/`install`/`postinstall`) is
    /// present — i.e. the package has an explicit install action.
    pub fn has_install_lifecycle(&self) -> bool {
        [
            LifecycleHook::Preinstall,
            LifecycleHook::Install,
            LifecycleHook::Postinstall,
        ]
        .iter()
        .any(|hook| self.scripts.contains_key(hook))
    }

    /// Whether an explicit `install` or `preinstall` hook is present. npm only
    /// defaults a `binding.gyp` package's install action to `node-gyp rebuild`
    /// when BOTH are absent, so either one suppresses the implicit native build.
    pub fn suppresses_default_node_gyp(&self) -> bool {
        self.scripts.contains_key(&LifecycleHook::Install)
            || self.scripts.contains_key(&LifecycleHook::Preinstall)
    }

    /// Insert/override one hook. Used to synthesize the implicit
    /// `node-gyp rebuild` install action for an allowed native package.
    pub fn set(&mut self, hook: LifecycleHook, script: String) {
        self.scripts.insert(hook, script);
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
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(rename = "publishConfig")]
    pub publish_config: PublishConfig,
}

impl PublishMeta {
    pub fn from_package_json(pkg: &PackageJson) -> Self {
        Self {
            private: pkg.private.unwrap_or(false),
            name: pkg.name.clone(),
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
        if self.name.is_empty() {
            bail!("Cannot publish: package.json requires a non-empty 'name' field");
        }
        if self.version.is_empty() {
            bail!("Cannot publish: package.json requires a non-empty 'version' field");
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
    /// Resolved version, used to match `allowScripts` `name@version` entries.
    /// Empty when unknown (e.g. project root, or the lock entry omits it).
    pub version: String,
    /// True when this entry is a workspace `node_modules` link. Its install
    /// lifecycle is owned by the workspace walk (`process_workspace_install_hooks`),
    /// so it is first-party and must never be gated as a third-party dependency
    /// by the `allowScripts` policy.
    pub is_workspace_link: bool,
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
    ///
    /// Project root and workspaces may legitimately omit `name` — npm allows
    /// running `install`/lifecycle scripts on `{}` package.json. Callers that
    /// actually require a name (publish, link-to-global) validate explicitly.
    pub fn from_package_json(path: &Path, pkg: &PackageJson) -> Result<Self> {
        Ok(PackageInfo {
            path: path.to_path_buf(),
            bin_files: pkg.bin_entries(),
            lifecycle_scripts: LifecycleScripts::from_scripts(pkg.scripts_or_empty()),
            scripts: pkg.scripts_or_empty().clone(),
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            // Project/workspace-source packages are loaded here and run via the
            // workspace walk, not gated as dependencies.
            is_workspace_link: false,
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
            // PackageInstallView omits version; install-script gating runs on the
            // lock-fed collect path, which sets version from the lock entry.
            version: String::new(),
            is_workspace_link: false,
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
            let global_bin_str = global_bin_dir.to_string_lossy().into_owned();
            if !current_path.contains(&global_bin_str) {
                let new_path = format!("{global_bin_str}{PATH_SEPARATOR}{current_path}");
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
        assert!(
            package_info
                .lifecycle_scripts
                .get_script(LifecycleHook::Preinstall)
                .is_some()
        );
        assert!(
            package_info
                .lifecycle_scripts
                .get_script(LifecycleHook::Install)
                .is_some()
        );
        assert!(
            package_info
                .lifecycle_scripts
                .get_script(LifecycleHook::Postinstall)
                .is_some()
        );
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
    async fn test_package_info_from_package_json_allows_missing_name() {
        // `utoo install <pkg>` against a `{}` package.json must succeed —
        // npm allows installing into an unnamed project, and the project
        // root never needs a name to run lifecycle hooks. Callers that do
        // need a name (publish, link-to-global) check explicitly.
        let pkg = PackageJson::default();
        let info = PackageInfo::from_package_json(Path::new("/tmp"), &pkg)
            .expect("project root without name must load");
        assert_eq!(info.name, "");
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
