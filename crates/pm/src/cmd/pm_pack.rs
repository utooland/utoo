use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::service::pm_pack as pack_service;
use crate::util::format_print::format_size;

pub async fn pack(path: Option<String>, dry_run: bool) -> Result<()> {
    let package_root = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        std::env::current_dir()?
    };

    let result = pack_service::pack(&package_root, dry_run).await?;

    for (f, size) in &result.files {
        println!("{} {f}", format_size(*size).dimmed());
    }
    println!();

    if dry_run {
        println!("{}", "(dry run) Tarball not created".yellow());
    } else if let Some(tp) = &result.tarball_path {
        println!("{} {}", "Tarball:".dimmed(), tp.display());
    }

    let row = |label: &str, val: &dyn std::fmt::Display| println!("{} {val}", label.dimmed());
    row("Name:", &result.name.cyan());
    row("Version:", &result.version);
    row("Files:", &result.files.len());
    row("Unpacked Size:", &format_size(result.unpacked_size));
    if !dry_run {
        row("Packed Size:", &format_size(result.packed_size));
        row("Integrity:", &result.integrity);
    }

    Ok(())
}
