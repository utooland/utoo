use crate::cmd::install::install;
use crate::helper::global_bin::{get_global_bin_dir, get_global_package_dir};
use crate::helper::workspace::update_cwd_to_project;
use crate::model::package::PackageInfo;
use crate::util::cli_enum::ScriptPolicy;
use crate::util::linker::link;
use crate::util::user_config::resolve_global_prefix;
use anyhow::{Context, Result};
use std::path::Path;

/// Link current package to global (equivalent to npm link without args)
pub async fn link_current_to_global(cwd: &Path, prefix: Option<&str>) -> Result<String> {
    // Resolve the effective prefix: CLI flag > UTOO_PREFIX env > config.
    let prefix = resolve_global_prefix(prefix).await;
    let prefix = prefix.as_deref();

    let project_path = update_cwd_to_project(cwd).await?;
    let package_info = PackageInfo::load(&project_path).await.with_context(|| {
        format!(
            "Failed to load package info from path: {}",
            project_path.display()
        )
    })?;
    if package_info.name.is_empty() {
        anyhow::bail!(
            "Cannot link to global: package.json at {} requires a non-empty 'name' field",
            project_path.display()
        );
    }

    // Install dependencies
    install(ScriptPolicy::Run, &project_path)
        .await
        .context("Failed to prepare dependencies for package to link")?;

    let global_package_path = get_global_package_dir(prefix)?.join(&package_info.name);
    // link local project to global package
    link(&project_path, &global_package_path)
        .await
        .with_context(|| format!("Failed to link {project_path:?} => {global_package_path:?}"))?;

    // If the package has binary files, link them into the global prefix bin dir
    // (`<prefix>/bin`, on PATH) — not the package's own bin dir.
    if package_info.has_bin_files() {
        let global_bin_dir =
            get_global_bin_dir(prefix).context("Failed to get global bin directory")?;
        package_info
            .link_to_global(&global_bin_dir)
            .await
            .with_context(|| {
                format!(
                    "Failed to link binary files to global bin directory: {}",
                    global_bin_dir.display()
                )
            })?;
    }

    Ok(package_info.name)
}

/// Link a global package to local node_modules (equivalent to npm link pkg_name)
pub async fn link_global_to_local(
    cwd: &Path,
    package_name: &str,
    prefix: Option<&str>,
) -> Result<()> {
    // Resolve the effective prefix: CLI flag > UTOO_PREFIX env > config.
    let prefix = resolve_global_prefix(prefix).await;
    let prefix = prefix.as_deref();

    let project_path = update_cwd_to_project(cwd).await?;

    let global_package_path = get_global_package_dir(prefix)?.join(package_name);

    let package_info = PackageInfo::load(&global_package_path)
        .await
        .with_context(|| {
            format!(
                "Failed to load package info from path: {}",
                global_package_path.display()
            )
        })?;

    // Create symlink from global package to local project
    let local_link_path = project_path.join("node_modules/").join(package_name);
    link(&global_package_path, &local_link_path)
        .await
        .with_context(|| {
            format!("Failed to link {global_package_path:?} => {local_link_path:?}")
        })?;

    tracing::debug!(
        "link global package to local: {} -> {}",
        global_package_path.display(),
        project_path.display()
    );

    // If the package has binary files, also link them to target project bin directory
    if package_info.has_bin_files() {
        tracing::debug!(
            "Linking binary files to project bin directory: {}",
            &package_info.name
        );
        let bin_dir = project_path.join("node_modules/.bin");
        package_info
            .link_to_target(&bin_dir)
            .await
            .with_context(|| {
                format!(
                    "Failed to link binary files to project bin directory: {}",
                    bin_dir.display()
                )
            })?;
    }

    Ok(())
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;
    use crate::util::platform_const::GLOBAL_NODE_MODULES;
    use std::fs;
    use tempfile::TempDir;

    // helper to write minimal package.json and lock to bypass install flow side effects
    async fn write_minimal_project_files(path: &std::path::Path, name: &str) {
        let pkg = format!(
            r#"{{
  "name": "{name}",
  "version": "1.0.0"
}}"#
        );
        fs::write(path.join("package.json"), pkg).unwrap();
        let lock = r#"{
  "name": "test",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {"name": "test", "version": "1.0.0"}
  }
}"#;
        fs::write(path.join("package-lock.json"), lock).unwrap();
        fs::create_dir_all(path.join("node_modules")).unwrap();
    }

    #[tokio::test]
    async fn test_link_global_to_local_basic() {
        let temp_dir = TempDir::new().unwrap();
        let project = temp_dir.path().join("proj");
        let global = temp_dir.path().join("global");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&global).unwrap();

        // minimal project files to bypass install
        write_minimal_project_files(&project, "consumer").await;

        // create fake global package dir with package.json
        let pkg_name = "lib-a";
        let global_pkg_dir = global.join(GLOBAL_NODE_MODULES).join(pkg_name);
        fs::create_dir_all(&global_pkg_dir).unwrap();
        let pkg_json = format!(
            r#"{{
  "name": "{pkg_name}",
  "version": "1.0.0"
}}"#
        );
        fs::write(global_pkg_dir.join("package.json"), pkg_json).unwrap();

        let prefix_str = global.to_string_lossy().into_owned();
        let result = link_global_to_local(&project, pkg_name, Some(&prefix_str)).await;
        assert!(result.is_ok(), "link_global_to_local should succeed");

        // verify node_modules symlink
        let local_link = project.join("node_modules").join(pkg_name);
        assert!(local_link.exists());
        assert!(local_link.is_symlink() || local_link.is_dir());
    }
}
