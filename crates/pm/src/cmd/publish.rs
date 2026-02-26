use anyhow::{Context, Result};
use colored::Colorize;

use crate::helper::workspace::update_cwd_to_project;
use crate::model::package::{PackageInfo, PublishMeta};
use crate::service::publish as publish_service;
use crate::util::json::load_package_json_from_path;
use crate::util::user_config::get_registry;

pub async fn publish(tag: Option<&str>, dry_run: bool, otp: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let package_root = update_cwd_to_project(&cwd).await?;
    let package_json = load_package_json_from_path(&package_root).await?;

    let meta = PublishMeta::from_json(&package_json);
    meta.validate()?;

    let tag = meta.resolve_tag(tag)?;
    let registry = meta
        .publish_config
        .registry
        .as_deref()
        .map(String::from)
        .unwrap_or_else(get_registry);

    let package_info = PackageInfo::from_json(&package_root, &package_json)?;
    let result = publish_service::publish(&package_info, &registry, &tag, dry_run, otp).await?;

    if dry_run {
        println!(
            "{}",
            format!(
                "(dry run) Would publish {}@{} to {} with tag '{}'",
                result.pack.name, result.pack.version, result.registry, result.tag
            )
            .yellow()
        );
    } else {
        println!(
            "{}",
            format!("+ {}@{}", result.pack.name, result.pack.version).green()
        );
    }

    Ok(())
}
