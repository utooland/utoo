use std::path::PathBuf;

use anyhow::Result;
use utoo_ruborist::util::{PackageNameStr, parse_package_spec};

use crate::fs;
use crate::util::cache::{get_cache_dir, get_self_pin_cache_dir, matches_pattern};
use crate::util::process_lock::{lock_exclusive, sibling_lock_path};

const SELF_PIN_LOGICAL_PREFIX: &str = "_utoo-self-";

fn reserves_self_pin_logical_namespace(pkg_pattern: &str) -> bool {
    pkg_pattern.starts_with(SELF_PIN_LOGICAL_PREFIX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheEntryKind {
    Package,
    SelfPin,
}

/// A cache entry slated for deletion.
#[derive(Debug)]
pub struct CacheEntry {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    kind: CacheEntryKind,
}

/// Collect every version directory of `pkg_name` under `path` whose version
/// matches `version_pattern`.
async fn collect_matching_versions(
    path: &std::path::Path,
    pkg_name: String,
    version_pattern: &str,
    kind: CacheEntryKind,
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
            to_delete.push(CacheEntry {
                name: pkg_name.clone(),
                version: version_str.to_string(),
                path: version_entry.path(),
                kind,
            });
        }
    }
    Ok(())
}

async fn collect_self_pin_cache_entries(
    root: &std::path::Path,
    pkg_pattern: &str,
    version_pattern: &str,
    to_delete: &mut Vec<CacheEntry>,
) -> Result<()> {
    if !fs::try_exists(root).await? {
        return Ok(());
    }
    let mut targets = fs::read_dir(root).await?;
    while let Some(target) = targets.next_entry().await? {
        if !target.file_type().await?.is_dir() {
            continue;
        }
        let target_name = target.file_name();
        let logical_name = format!("{SELF_PIN_LOGICAL_PREFIX}{}", target_name.to_string_lossy());
        if matches_pattern(&logical_name, pkg_pattern) {
            collect_matching_versions(
                &target.path(),
                logical_name,
                version_pattern,
                CacheEntryKind::SelfPin,
                to_delete,
            )
            .await?;
        }
    }
    Ok(())
}

/// Walk the package cache and the dedicated self-pin cache root, collecting
/// entries matching `pattern`
/// (a `name[@version]` spec, both parts may contain wildcards).
///
/// Handles scoped (`@scope/name`) and regular packages; the result is sorted
/// by package name and version number.
async fn collect_cache_entries_at(
    cache_dir: &std::path::Path,
    self_pin_cache_dir: &std::path::Path,
    pattern: &str,
) -> Result<Vec<CacheEntry>> {
    let (pkg_pattern, version_pattern) = parse_package_spec(pattern);
    let mut to_delete = Vec::new();

    collect_self_pin_cache_entries(
        self_pin_cache_dir,
        pkg_pattern,
        version_pattern,
        &mut to_delete,
    )
    .await?;

    // The `_utoo-self-` prefix is reserved by this command for the logical
    // view of the dedicated sibling root. This keeps exact clean operations
    // from also deleting a permissive private-registry package with that name.
    if !reserves_self_pin_logical_namespace(pkg_pattern) {
        // Read all package information
        let mut entries = if fs::try_exists(&cache_dir).await? {
            Some(fs::read_dir(&cache_dir).await?)
        } else {
            None
        };
        while let Some(entry) = match &mut entries {
            Some(entries) => entries.next_entry().await?,
            None => None,
        } {
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
                            CacheEntryKind::Package,
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
                        CacheEntryKind::Package,
                        &mut to_delete,
                    )
                    .await?;
                }
            }
        }
    }

    // Sort by package name and version number
    to_delete.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));

    Ok(to_delete)
}

pub async fn collect_cache_entries(pattern: &str) -> Result<Vec<CacheEntry>> {
    collect_cache_entries_at(&get_cache_dir(), &get_self_pin_cache_dir()?, pattern).await
}

/// Delete the given cache entries from global storage. Failures are logged
/// per entry and do not abort the remaining deletions.
pub async fn delete_cache_entries(
    to_delete: Vec<CacheEntry>,
) -> (Vec<CacheEntry>, Vec<(CacheEntry, std::io::Error)>) {
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for entry in to_delete {
        let _self_pin_lock = if entry.kind == CacheEntryKind::SelfPin {
            let lock_result = match sibling_lock_path(&entry.path, ".self-pin.lock") {
                Ok(lock_path) => lock_exclusive(&lock_path).await,
                Err(error) => Err(error),
            };
            match lock_result {
                Ok(lock) => Some(lock),
                Err(e) => {
                    let e = std::io::Error::other(e.to_string());
                    tracing::error!(
                        "Failed to lock {}@{} for deletion: {e}",
                        entry.name,
                        entry.version
                    );
                    failed.push((entry, e));
                    continue;
                }
            }
        } else {
            None
        };
        if let Err(e) = fs::remove_dir_all(&entry.path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::error!("Failed to delete {}@{}: {e}", entry.name, entry.version);
            failed.push((entry, e));
        } else {
            tracing::debug!("Deleted {}@{}", entry.name, entry.version);
            deleted.push(entry);
        }
    }
    (deleted, failed)
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
            CacheEntryKind::Package,
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].name, "test-pkg");
        assert_eq!(to_delete[0].version, "1.0.0");
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
            CacheEntryKind::Package,
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 2);
        assert!(to_delete.iter().any(|x| x.version == "1.0.0"));
        assert!(to_delete.iter().any(|x| x.version == "1.0.1"));
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_matching_versions_empty_dir() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut to_delete = Vec::new();

        collect_matching_versions(
            temp_dir.path(),
            "test-pkg".to_string(),
            "*",
            CacheEntryKind::Package,
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_matching_versions_ignores_lock_files() -> Result<()> {
        let temp_dir = setup_test_dir().await?;
        fs::write(temp_dir.path().join(".1.0.0.lock"), b"").await?;
        let mut to_delete = Vec::new();

        collect_matching_versions(
            temp_dir.path(),
            "test-pkg".to_string(),
            "*",
            CacheEntryKind::Package,
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 4);
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_self_pin_versions_uses_clean_logical_names() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let self_pin_root = temp_dir.path().join("nm.utoo-self-pin");
        fs::create_dir_all(self_pin_root.join("darwin-arm64/1.1.8")).await?;
        fs::create_dir_all(self_pin_root.join("darwin-x64/1.1.8")).await?;
        fs::write(self_pin_root.join("darwin-arm64/.1.1.8.self-pin.lock"), b"").await?;
        let mut to_delete = Vec::new();

        collect_self_pin_cache_entries(
            &self_pin_root,
            "_utoo-self-darwin-arm64",
            "1.1.8",
            &mut to_delete,
        )
        .await?;

        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].name, "_utoo-self-darwin-arm64");
        assert_eq!(to_delete[0].version, "1.1.8");
        assert_eq!(to_delete[0].kind, CacheEntryKind::SelfPin);
        assert_eq!(to_delete[0].path, self_pin_root.join("darwin-arm64/1.1.8"),);
        assert!(reserves_self_pin_logical_namespace(
            "_utoo-self-darwin-arm64"
        ));
        assert!(reserves_self_pin_logical_namespace("_utoo-self-*"));
        assert!(!reserves_self_pin_logical_namespace("*"));
        Ok(())
    }

    #[tokio::test]
    async fn test_collect_self_pin_versions_without_package_cache_root() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let package_cache = temp_dir.path().join("missing-nm");
        let self_pin_root = temp_dir.path().join("missing-nm.utoo-self-pin");
        fs::create_dir_all(self_pin_root.join("darwin-arm64/1.1.8")).await?;
        let to_delete = collect_cache_entries_at(
            &package_cache,
            &self_pin_root,
            "_utoo-self-darwin-arm64@1.1.8",
        )
        .await?;

        assert!(!fs::try_exists(package_cache).await?);
        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].kind, CacheEntryKind::SelfPin);
        Ok(())
    }

    #[tokio::test]
    async fn test_clean_wildcard_keeps_private_registry_entry_provenance() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let package_cache = temp_dir.path().join("nm");
        let self_pin_root = temp_dir.path().join("nm.utoo-self-pin");
        let version_path = package_cache.join("_utoo-self-private/1.0.0");
        fs::create_dir_all(&version_path).await?;

        let to_delete = collect_cache_entries_at(&package_cache, &self_pin_root, "*").await?;
        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0].kind, CacheEntryKind::Package);

        let (deleted, failed) = delete_cache_entries(to_delete).await;
        assert_eq!(deleted.len(), 1);
        assert!(failed.is_empty());
        assert!(!fs::try_exists(&version_path).await?);
        assert!(
            !fs::try_exists(package_cache.join("_utoo-self-private/.1.0.0.self-pin.lock")).await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_self_pin_delete_waits_for_handoff_lock() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let version_path = temp_dir.path().join("darwin-arm64/1.1.8");
        fs::create_dir_all(&version_path).await?;
        let lock_path = sibling_lock_path(&version_path, ".self-pin.lock")?;
        let handoff_lock = lock_exclusive(&lock_path).await?;

        let entry = CacheEntry {
            name: "_utoo-self-darwin-arm64".to_string(),
            version: "1.1.8".to_string(),
            path: version_path.clone(),
            kind: CacheEntryKind::SelfPin,
        };
        let mut deletion = tokio::spawn(delete_cache_entries(vec![entry]));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut deletion)
                .await
                .is_err(),
            "clean unexpectedly completed while the handoff lock was held"
        );
        assert!(fs::try_exists(&version_path).await?);

        drop(handoff_lock);
        let (deleted, failed) =
            tokio::time::timeout(std::time::Duration::from_secs(5), deletion).await??;
        assert_eq!(deleted.len(), 1);
        assert!(failed.is_empty());
        assert!(!fs::try_exists(&version_path).await?);
        Ok(())
    }
}
