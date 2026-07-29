use std::path::PathBuf;

use anyhow::Result;
use utoo_ruborist::util::{PackageNameStr, parse_package_spec};

use crate::fs;
use crate::util::cache::{get_cache_dir, matches_pattern};

/// A cache entry slated for deletion: (package name, version, on-disk path).
pub type CacheEntry = (String, String, PathBuf);

/// Collect every version directory of `pkg_name` under `path` whose version
/// matches `version_pattern`, appending `(pkg_name, version, path)` to
/// `to_delete`.
async fn collect_matching_versions(
    path: &std::path::Path,
    pkg_name: String,
    version_pattern: &str,
    to_delete: &mut Vec<CacheEntry>,
) -> Result<()> {
    let mut version_entries = fs::read_dir(path).await?;
    while let Some(version_entry) = version_entries.next_entry().await? {
        if !version_entry.file_type().await?.is_dir() {
            continue;
        }
        let version = version_entry.file_name();
        let version_str = version.to_string_lossy();
        if matches_pattern(&version_str, version_pattern) {
            to_delete.push((
                pkg_name.clone(),
                version_str.to_string(),
                version_entry.path(),
            ));
        }
    }
    Ok(())
}

/// Walk the global cache directory and collect entries matching `pattern`
/// (a `name[@version]` spec, both parts may contain wildcards).
///
/// Handles scoped (`@scope/name`) and regular packages; the result is sorted
/// by package name and version number.
pub async fn collect_cache_entries(pattern: &str) -> Result<Vec<CacheEntry>> {
    let cache_dir = get_cache_dir();

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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn setup_test_dir() -> Result<TempDir> {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create test version directories
        fs::create_dir_all(base_path.join("1.0.0")).await?;
        fs::create_dir_all(base_path.join("1.0.1")).await?;
        fs::create_dir_all(base_path.join("2.0.0")).await?;
        fs::create_dir_all(base_path.join("beta-1.0.0")).await?;

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn test_collect_matching_versions_exact_match() -> Result<()> {
        let temp_dir = setup_test_dir().await?;
        let mut to_delete = Vec::new();

        collect_matching_versions(
            temp_dir.path(),
            "test-pkg".to_string(),
            "1.0.0",
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].0, "test-pkg");
        assert_eq!(to_delete[0].1, "1.0.0");
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_matching_versions_wildcard() -> Result<()> {
        let temp_dir = setup_test_dir().await?;
        let mut to_delete = Vec::new();

        collect_matching_versions(
            temp_dir.path(),
            "test-pkg".to_string(),
            "1.*",
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 2);
        assert!(to_delete.iter().any(|x| x.1 == "1.0.0"));
        assert!(to_delete.iter().any(|x| x.1 == "1.0.1"));
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_matching_versions_empty_dir() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut to_delete = Vec::new();

        collect_matching_versions(temp_dir.path(), "test-pkg".to_string(), "*", &mut to_delete)
            .await?;

        assert_eq!(to_delete.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_matching_versions_ignores_lock_files() -> Result<()> {
        let temp_dir = setup_test_dir().await?;
        fs::write(temp_dir.path().join(".1.0.0.lock"), b"").await?;
        let mut to_delete = Vec::new();

        collect_matching_versions(temp_dir.path(), "test-pkg".to_string(), "*", &mut to_delete)
            .await?;

        assert_eq!(to_delete.len(), 4);
        Ok(())
    }
}
