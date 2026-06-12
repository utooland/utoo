use std::path::PathBuf;

use anyhow::{Context, Result};
use utoo_ruborist::util::{PackageNameStr, parse_package_spec};

use crate::fs;
use crate::util::cache::{collect_matching_versions, matches_pattern};

/// A cache entry slated for deletion: (package name, version, on-disk path).
pub type CacheEntry = (String, String, PathBuf);

/// Walk the global cache directory and collect entries matching `pattern`
/// (a `name[@version]` spec, both parts may contain wildcards).
///
/// Handles scoped (`@scope/name`) and regular packages; the result is sorted
/// by package name and version number.
pub async fn collect_cache_entries(pattern: &str) -> Result<Vec<CacheEntry>> {
    let cache_dir = dirs::home_dir()
        .map(|p| p.join(".cache").join("nm"))
        .context("Failed to get cache directory")?;

    let (pkg_pattern, version_pattern) = parse_package_spec(pattern);
    let mut to_delete = Vec::new();

    // Read all package information
    let mut entries = fs::read_dir(&cache_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.is_scoped() {
            // Handle scoped packages
            let mut pkg_entries = fs::read_dir(entry.path()).await?;
            while let Some(pkg_entry) = pkg_entries.next_entry().await? {
                let pkg_name = pkg_entry.file_name();
                let full_pkg_name = format!("{}/{}", name_str, pkg_name.to_string_lossy());

                if matches_pattern(&full_pkg_name, pkg_pattern) {
                    tracing::debug!("full pkg name {full_pkg_name}, pkg_pattern {pkg_pattern}");
                    collect_matching_versions(
                        &pkg_entry.path(),
                        full_pkg_name,
                        version_pattern,
                        &mut to_delete,
                    )
                    .await?;
                }
            }
        } else {
            // Handle regular packages
            if matches_pattern(&name_str, pkg_pattern) {
                collect_matching_versions(
                    &entry.path(),
                    name_str.to_string(),
                    version_pattern,
                    &mut to_delete,
                )
                .await?;
            }
        }
    }

    // Sort by package name and version number
    to_delete.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    Ok(to_delete)
}

/// Delete the given cache entries from global storage. Failures are logged
/// per entry and do not abort the remaining deletions.
pub async fn delete_cache_entries(to_delete: Vec<CacheEntry>) {
    for (pkg, version, path) in to_delete {
        if let Err(e) = fs::remove_dir_all(&path).await {
            tracing::error!("Failed to delete {pkg}@{version}: {e}");
        } else {
            tracing::debug!("Deleted {pkg}@{version}");
        }
    }
}
