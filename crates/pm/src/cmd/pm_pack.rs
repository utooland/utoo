use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

use crate::service::pm_pack as pack_service;
use crate::util::format_print::print_pack_details;

pub async fn pack(path: Option<String>, dry_run: bool) -> Result<()> {
    let package_root = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        std::env::current_dir()?
    };

    let result = pack_service::pack(&package_root, dry_run).await?;

    print_pack_details(&result, None);

    if dry_run {
        println!("{}", "(dry run) Tarball not created".yellow());
    } else if let Some(tar_data) = &result.tarball_data {
        let tarball_name = format!(
            "{}-{}.tgz",
            result.name.replace('/', "-").replace('@', ""),
            result.version
        );
        let tarball_path = package_root.join(&tarball_name);
        crate::fs::write(&tarball_path, tar_data)
            .await
            .with_context(|| format!("Failed to write tarball to {}", tarball_path.display()))?;
        println!("{} {}", "Tarball:".dimmed(), tarball_path.display());
    }

    Ok(())
}
