use anyhow::{Context, Result};
use colored::Colorize;

use crate::helper::workspace::update_cwd_to_project;
use crate::model::package::PackageInfo;
use crate::service::publish as publish_service;
use crate::util::json::load_package_json_from_path;
use crate::util::user_config::get_registry;

/// Fields read from `publishConfig` in package.json.
#[derive(Default, serde::Deserialize)]
struct PublishConfig {
    tag: Option<String>,
    registry: Option<String>,
}

pub async fn publish(tag: Option<&str>, dry_run: bool, otp: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let package_root = update_cwd_to_project(&cwd).await?;

    let package_json = load_package_json_from_path(&package_root).await?;
    let publish_config: PublishConfig = package_json
        .get("publishConfig")
        .and_then(|pc| serde_json::from_value(pc.clone()).ok())
        .unwrap_or_default();

    // Resolve tag: --tag CLI flag > publishConfig.tag > "latest"
    let tag = tag
        .map(String::from)
        .or(publish_config.tag)
        .unwrap_or_else(|| "latest".to_string());

    // Resolve registry: publishConfig.registry > global config
    let registry = publish_config.registry.unwrap_or_else(get_registry);

    let package_info = PackageInfo::from_json(&package_root, &package_json)?;

    let result = publish_service::publish(&package_info, &registry, &tag, dry_run, otp).await?;

    if dry_run {
        println!(
            "{}",
            format!(
                "(dry run) Would publish {}@{} to {} with tag '{}'",
                result.name, result.version, result.registry, result.tag
            )
            .yellow()
        );
    } else {
        println!(
            "{}",
            format!("+ {}@{}", result.name, result.version).green()
        );
    }

    Ok(())
}
