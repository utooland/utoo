use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::service::pack as pack_service;

pub async fn pack(path: Option<String>, dry_run: bool, json: bool) -> Result<()> {
    let package_root = match path {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir()?,
    };

    let result = pack_service::pack(&package_root, dry_run).await?;

    if json {
        let json_output = serde_json::json!({
            "name": result.name,
            "version": result.version,
            "filename": result.tarball_path.as_ref().map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string()),
            "files": result.files.iter().map(|f| {
                serde_json::json!({ "path": f.path, "size": f.size })
            }).collect::<Vec<_>>(),
            "entryCount": result.file_count,
            "shasum": result.shasum,
            "integrity": result.integrity,
            "unpackedSize": result.unpacked_size,
            "packedSize": result.packed_size,
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
        return Ok(());
    }

    // Print file list
    for f in &result.files {
        println!("{}", f.path);
    }
    println!();

    if dry_run {
        println!("{}", "(dry run) Tarball not created".yellow());
    } else if let Some(tarball_path) = &result.tarball_path {
        println!("{} {}", "Tarball:".dimmed(), tarball_path.display());
    }

    println!("{} {}", "Name:".dimmed(), result.name.cyan());
    println!("{} {}", "Version:".dimmed(), result.version);
    println!("{} {}", "Files:".dimmed(), result.file_count);
    println!(
        "{} {}",
        "Unpacked Size:".dimmed(),
        format_size(result.unpacked_size)
    );

    if !dry_run {
        println!(
            "{} {}",
            "Packed Size:".dimmed(),
            format_size(result.packed_size)
        );
        println!("{} {}", "Shasum:".dimmed(), result.shasum);
        println!("{} {}", "Integrity:".dimmed(), result.integrity);
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
