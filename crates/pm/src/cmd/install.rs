use anyhow::Result;

use crate::service::install::InstallService;
use crate::util::save_type::{PackageAction, SaveType};

pub async fn update_packages(
    action: PackageAction,
    specs: &[&str],
    workspace: Option<String>,
    ignore_scripts: bool,
    save_type: SaveType,
) -> Result<()> {
    // Parameter validation
    if specs.is_empty() {
        return Err(anyhow::anyhow!("No package specifications provided"));
    }

    // Dispatch to service
    InstallService::update_packages(action, specs, workspace, ignore_scripts, save_type).await
}

pub async fn install(ignore_scripts: bool, root_path: &std::path::Path) -> Result<()> {
    // Dispatch to service
    InstallService::install(ignore_scripts, root_path).await
}

pub async fn install_global_package(npm_spec: &str, prefix: Option<&str>) -> Result<()> {
    // Parameter validation
    if npm_spec.trim().is_empty() {
        return Err(anyhow::anyhow!("Package specification cannot be empty"));
    }

    // Dispatch to service
    InstallService::install_global_package(npm_spec, prefix).await
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
        assert!(result.is_err(), "Should fail with whitespace-only package spec");
    }

    #[tokio::test]
    async fn test_update_packages_empty_specs() {
        // Test update with empty specs
        let result = update_packages(
            PackageAction::Add,
            &[],
            None,
            false,
            SaveType::Prod,
        ).await;
        assert!(result.is_err(), "Should fail with empty specs");
    }
}
