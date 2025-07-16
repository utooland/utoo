use anyhow::{Context, Result};
use std::io::{self, Write};
use tokio::fs;

use crate::util::{
    cache::{collect_matching_versions, matches_pattern, parse_pattern},
    logger::{log_error, log_info, log_verbose},
};

pub async fn clean(pattern: &str) -> Result<()> {
    let cache_dir = dirs::home_dir()
        .map(|p| p.join(".cache").join("nm"))
        .context("Failed to get cache directory")?;

    let (pkg_pattern, version_pattern) = parse_pattern(pattern);
    let mut to_delete = Vec::new();

    // Read all package information
    let mut entries = fs::read_dir(&cache_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('@') {
            // Handle scoped packages
            let mut pkg_entries = fs::read_dir(entry.path()).await?;
            while let Some(pkg_entry) = pkg_entries.next_entry().await? {
                let pkg_name = pkg_entry.file_name();
                let full_pkg_name = format!("{}/{}", name_str, pkg_name.to_string_lossy());

                if matches_pattern(&full_pkg_name, &pkg_pattern) {
                    log_verbose(&format!(
                        "full pkg name {full_pkg_name}, pkg_pattern {pkg_pattern}"
                    ));
                    collect_matching_versions(
                        &pkg_entry.path(),
                        full_pkg_name,
                        &version_pattern,
                        &mut to_delete,
                    )
                    .await?;
                }
            }
        } else {
            // Handle regular packages
            if matches_pattern(&name_str, &pkg_pattern) {
                collect_matching_versions(
                    &entry.path(),
                    name_str.to_string(),
                    &version_pattern,
                    &mut to_delete,
                )
                .await?;
            }
        }
    }

    if to_delete.is_empty() {
        log_info("No matching cache files found");
        return Ok(());
    }

    // Sort by package name and version number
    to_delete.sort_by(|a, b| {
        let pkg_cmp = a.0.cmp(&b.0);
        if pkg_cmp == std::cmp::Ordering::Equal {
            a.1.cmp(&b.1)
        } else {
            pkg_cmp
        }
    });

    println!("\nThe following caches will be deleted:");
    for (pkg, version, _) in &to_delete {
        println!("- {pkg}@{version}");
    }

    println!();
    log_info(
        "Note: This will only delete caches from global storage and won't affect dependencies in the current project. If you need to reinstall project dependencies, please run 'utoo update'",
    );
    print!(
        "\nConfirm to delete these {} packages? [y/N] ",
        to_delete.len()
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() == "y" {
        for (pkg, version, path) in to_delete {
            if let Err(e) = fs::remove_dir_all(&path).await {
                log_error(&format!("Failed to delete {pkg}@{version}: {e}"));
            } else {
                log_verbose(&format!("Deleted {pkg}@{version}"));
            }
        }
        log_info("Cleanup completed");
    }

    Ok(())
}
