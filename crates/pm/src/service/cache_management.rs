use anyhow::{Context, Result};
use std::io::{self, Write};
use tokio::fs;

use crate::util::{
    cache::{collect_matching_versions, matches_pattern, parse_pattern},
    logger::{log_error, log_info, log_verbose},
};

pub struct CacheManagementService;

impl CacheManagementService {
    pub async fn clean_cache(pattern: &str) -> Result<()> {
        let cache_dir = Self::get_cache_directory()?;
        let to_delete = Self::find_matching_cache_entries(&cache_dir, pattern).await?;
        
        if to_delete.is_empty() {
            log_info("No matching cache entries found");
            return Ok(());
        }

        Self::confirm_and_delete_entries(to_delete).await
    }

    fn get_cache_directory() -> Result<std::path::PathBuf> {
        dirs::home_dir()
            .map(|p| p.join(".cache").join("nm"))
            .context("Failed to get cache directory")
    }

    async fn find_matching_cache_entries(
        cache_dir: &std::path::Path,
        pattern: &str,
    ) -> Result<Vec<(String, String, std::path::PathBuf)>> {
        let (pkg_pattern, version_pattern) = parse_pattern(pattern);
        let mut to_delete = Vec::new();

        let mut entries = fs::read_dir(cache_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with('@') {
                Self::process_scoped_packages(
                    &entry.path(),
                    &name_str,
                    &pkg_pattern,
                    &version_pattern,
                    &mut to_delete,
                )
                .await?;
            } else {
                Self::process_regular_package(
                    &entry.path(),
                    &name_str,
                    &pkg_pattern,
                    &version_pattern,
                    &mut to_delete,
                )
                .await?;
            }
        }

        Ok(to_delete)
    }

    async fn process_scoped_packages(
        entry_path: &std::path::Path,
        name_str: &str,
        pkg_pattern: &str,
        version_pattern: &str,
        to_delete: &mut Vec<(String, String, std::path::PathBuf)>,
    ) -> Result<()> {
        let mut pkg_entries = fs::read_dir(entry_path).await?;
        while let Some(pkg_entry) = pkg_entries.next_entry().await? {
            let pkg_name = pkg_entry.file_name();
            let full_pkg_name = format!("{}/{}", name_str, pkg_name.to_string_lossy());

            if matches_pattern(&full_pkg_name, pkg_pattern) {
                log_verbose(&format!(
                    "full pkg name {full_pkg_name}, pkg_pattern {pkg_pattern}"
                ));
                collect_matching_versions(
                    &pkg_entry.path(),
                    full_pkg_name,
                    version_pattern,
                    to_delete,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn process_regular_package(
        entry_path: &std::path::Path,
        name_str: &str,
        pkg_pattern: &str,
        version_pattern: &str,
        to_delete: &mut Vec<(String, String, std::path::PathBuf)>,
    ) -> Result<()> {
        if matches_pattern(name_str, pkg_pattern) {
            collect_matching_versions(
                entry_path,
                name_str.to_string(),
                version_pattern,
                to_delete,
            )
            .await?;
        }
        Ok(())
    }

    async fn confirm_and_delete_entries(
        to_delete: Vec<(String, String, std::path::PathBuf)>,
    ) -> Result<()> {
        // Show what would be deleted
        log_info(&format!("Found {} matching cache entries:", to_delete.len()));
        for (name, version, path) in &to_delete {
            log_info(&format!("  {}@{}: {}", name, version, path.display()));
        }

        // Ask for confirmation
        if !Self::confirm_deletion()? {
            log_info("Cache cleanup cancelled");
            return Ok(());
        }

        // Delete the directories
        for (name, version, path) in to_delete {
            log_verbose(&format!("Deleting cache entry: {}@{}", name, version));
            if let Err(e) = fs::remove_dir_all(&path).await {
                log_error(&format!("Failed to delete {}@{}: {}", name, version, e));
            } else {
                log_info(&format!("Deleted: {}@{}", name, version));
            }
        }

        log_info("Cache cleanup completed");
        Ok(())
    }

    fn confirm_deletion() -> Result<bool> {
        print!("Do you want to delete these cache entries? (y/N): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        Ok(input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes")
    }
}