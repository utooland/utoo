use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{self, Write};

use crate::helper::workspace::update_cwd_to_project;
use crate::model::RunMode;
use crate::model::package::{PackageInfo, PublishMeta};
use crate::service::publish::{self as publish_service, PublishOptions};
use crate::util::user_config::{get_or_load_package_json, get_registry};

pub async fn publish(tag: Option<&str>, mode: RunMode, otp: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let package_root = update_cwd_to_project(&cwd).await?;
    let pkg = get_or_load_package_json(&package_root).await?;

    let meta = PublishMeta::from_package_json(&pkg);
    meta.validate()?;

    let tag = meta.resolve_tag(tag)?;
    let registry = meta
        .publish_config
        .registry
        .as_deref()
        .map(String::from)
        .unwrap_or_else(get_registry);
    let package_info = PackageInfo::from_package_json(&package_root, &pkg)?;
    let result = publish_service::publish(&PublishOptions {
        package_info: &package_info,
        registry: &registry,
        tag: &tag,
        mode,
        otp,
    })
    .await?;

    let mut stdout = io::stdout().lock();
    if mode == RunMode::DryRun {
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
