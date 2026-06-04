use crate::util::cli_enum::ScriptPolicy;
use anyhow::Result;
use std::path::Path;

use crate::service::install::InstallService;
use crate::util::cli_enum::{PackageAction, SaveType};
use crate::util::user_config::get_omit;

pub async fn update_packages(
    action: PackageAction,
    specs: &[&str],
    workspace: Option<String>,
    scripts: ScriptPolicy,
    save_type: SaveType,
) -> Result<()> {
    // Parameter validation
    if specs.is_empty() {
        return Err(anyhow::anyhow!("No package specifications provided"));
    }

    let omit = get_omit();
    InstallService::update_packages(action, specs, workspace, scripts, save_type, &omit).await
}

pub async fn install(scripts: ScriptPolicy, root_path: &Path) -> Result<()> {
    let omit = get_omit();
    InstallService::install(scripts, root_path, &omit).await
}

pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
    // Parameter validation
    if npm_spec.trim().is_empty() {
        return Err(anyhow::anyhow!("Package specification cannot be empty"));
    }

    // Resolve the effective prefix: CLI flag > UTOO_PREFIX env > config.
    let prefix = crate::util::user_config::resolve_global_prefix(prefix).await;

    // Dispatch to service
    InstallService::install_global_package(npm_spec, prefix.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_install_global_package_empty_spec() {
        // Test installing with empty package spec
        let result = install_global_package("", None).await;
        assert!(result.is_err(), "Should fail with empty package spec");

        let result = install_global_package("   ", None).await;
        assert!(
            result.is_err(),
            "Should fail with whitespace-only package spec"
        );
    }

    #[tokio::test]
    async fn test_update_packages_empty_specs() {
        // Test update with empty specs
        let result = update_packages(
            PackageAction::Add,
            &[],
            None,
            ScriptPolicy::Run,
            SaveType::Prod,
        )
        .await;
        assert!(result.is_err(), "Should fail with empty specs");
    }
}
