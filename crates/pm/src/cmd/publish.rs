use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{self, Write};

use crate::helper::workspace::update_cwd_to_project;
use crate::model::package::{PackageInfo, PublishMeta};
use crate::service::publish::{self as publish_service, PublishOptions};
use crate::util::json::load_package_json_from_path;
use crate::util::user_config::get_registry;

pub async fn publish(tag: Option<&str>, dry_run: bool, otp: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let package_root = update_cwd_to_project(&cwd).await?;
    let package_json = load_package_json_from_path(&package_root).await?;

    let meta = PublishMeta::from_json(&package_json);
    meta.validate()?;

    let tag = meta.resolve_tag(tag)?;
    let registry = match meta.publish_config.registry.as_deref() {
        Some(r) => r.to_string(),
        None => get_registry().await,
    };
    let package_info = PackageInfo::from_json(&package_root, &package_json)?;
    let result = publish_service::publish(&PublishOptions {
        package_info: &package_info,
        registry: &registry,
        tag: &tag,
        mode: dry_run.into(),
        otp,
    })
    .await?;

    let mut stdout = io::stdout().lock();
    if dry_run {
        writeln!(
            stdout,
            "{}",
            format!(
                "(dry run) Would publish {}@{} to {} with tag '{}'",
                result.pack.name, result.pack.version, result.registry, result.tag
            )
            .yellow()
        )?;
    } else {
        writeln!(
            stdout,
            "{}",
            format!("+ {}@{}", result.pack.name, result.pack.version).green()
        )?;
    }

    Ok(())
}
