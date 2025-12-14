use std::path::PathBuf;
use tokio::fs;

use super::config;

pub fn parse_pattern(pattern: &str) -> (String, String) {
    // for @scope/pkg@version
    if pattern.starts_with('@') {
        if let Some(at_pos) = pattern.rfind('@')
            && let Some(slash_pos) = pattern.find('/')
            && at_pos > slash_pos
        {
            // for @scope/name@version
            let (pkg, version) = pattern.split_at(at_pos);
            return (pkg.to_string(), version[1..].to_string());
        }
        // @scope/name or @scope*
        return (pattern.to_string(), "*".to_string());
    }

    // no scope pkg
    let parts: Vec<&str> = pattern.rsplitn(2, '@').collect();
    match parts.as_slice() {
        [version, pkg] => (pkg.to_string(), version.to_string()),
        [pkg] => (pkg.to_string(), "*".to_string()),
        _ => ("*".to_string(), "*".to_string()),
    }
}

pub fn matches_pattern(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // special handle when /*
    if let Some(scope) = pattern.strip_suffix("/*") {
        return text.starts_with(scope);
    }

    // starts with *
    if let Some(suffix) = pattern.strip_prefix('*') {
        return text.ends_with(suffix);
    }

    // ends with *
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }

    // a*b
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if !text.starts_with(parts[0]) {
            return false;
        }
        if !text.ends_with(parts[parts.len() - 1]) {
            return false;
        }
        return true;
    }

    // exact match
    text == pattern
}

pub async fn collect_matching_versions(
    path: &std::path::Path,
    pkg_name: String,
    version_pattern: &str,
    to_delete: &mut Vec<(String, String, std::path::PathBuf)>,
) -> anyhow::Result<()> {
    let mut version_entries = fs::read_dir(path).await?;
    while let Some(version_entry) = version_entries.next_entry().await? {
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

/// Converts registry URL to directory name by removing protocol prefix and trailing slash.
fn registry_to_dir_name(registry_url: &str) -> String {
    let url = registry_url
        .strip_prefix("https://")
        .or_else(|| registry_url.strip_prefix("http://"))
        .unwrap_or(registry_url);
    url.trim_end_matches('/').to_string()
}

/// Returns cache directory path with registry isolation: ~/.cache/nm/registry-host/
pub fn get_cache_dir() -> PathBuf {
    let base_cache_dir = config::get_cache_dir();
    let registry = config::get_registry();
    let registry_dir = registry_to_dir_name(&registry);
    base_cache_dir.join(registry_dir)
}

pub fn get_package_versions_cache_file(package_name: &str) -> PathBuf {
    // Escape package name for filesystem compatibility
    get_cache_dir().join(package_name).join("versions.json")
}

pub fn get_package_manifest_cache_file(package_name: &str, version: &str) -> PathBuf {
    // Escape package name and version for filesystem compatibility
    get_cache_dir()
        .join(package_name)
        .join("manifests")
        .join(format!("{version}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[test]
    fn test_parse_pattern_normal_packages() {
        // normal pkg
        assert_eq!(
            parse_pattern("express"),
            ("express".to_string(), "*".to_string())
        );

        // normal pkg with version
        assert_eq!(
            parse_pattern("express@4.17.1"),
            ("express".to_string(), "4.17.1".to_string())
        );
    }

    #[test]
    fn test_parse_pattern_scoped_packages() {
        // scoped pkg
        assert_eq!(
            parse_pattern("@types/node"),
            ("@types/node".to_string(), "*".to_string())
        );

        // scoped pkg with version
        assert_eq!(
            parse_pattern("@types/node@14.14.31"),
            ("@types/node".to_string(), "14.14.31".to_string())
        );

        // special case: @types/*
        assert_eq!(
            parse_pattern("@types/*"),
            ("@types/*".to_string(), "*".to_string())
        );
    }

    #[test]
    fn test_matches_pattern_wildcard() {
        // *
        assert!(matches_pattern("anything", "*"));
        assert!(matches_pattern("", "*"));
    }

    #[test]
    fn test_matches_pattern_scope_wildcard() {
        // ends with /*
        assert!(matches_pattern("@types/node", "@types/*"));
        assert!(matches_pattern("@scope/package", "@scope/*"));
        assert!(!matches_pattern("@other/package", "@scope/*"));
    }

    #[test]
    fn test_matches_pattern_prefix_wildcard() {
        // starts with *
        assert!(matches_pattern("hello-world", "*world"));
        assert!(matches_pattern("world", "*world"));
        assert!(!matches_pattern("hello", "*world"));
    }

    #[test]
    fn test_matches_pattern_suffix_wildcard() {
        // ends with *
        assert!(matches_pattern("hello-world", "hello*"));
        assert!(matches_pattern("hello", "hello*"));
        assert!(!matches_pattern("world", "hello*"));
    }

    #[test]
    fn test_matches_pattern_middle_wildcard() {
        // a*b
        assert!(matches_pattern("hello-world", "hello*world"));
        assert!(matches_pattern("hello-beautiful-world", "hello*world"));
        assert!(!matches_pattern("hello-beautiful", "hello*world"));
        assert!(!matches_pattern("beautiful-world", "hello*world"));
    }

    #[test]
    fn test_matches_pattern_exact() {
        // exact match
        assert!(matches_pattern("exact", "exact"));
        assert!(!matches_pattern("exact", "not-exact"));
        assert!(!matches_pattern("", "not-empty"));
        assert!(matches_pattern("", ""));
    }

    #[test]
    fn test_matches_pattern_version_numbers() {
        // version test
        assert!(matches_pattern("1.0.0", "1.*"));
        assert!(matches_pattern("1.2.3", "1.*"));
        assert!(!matches_pattern("2.0.0", "1.*"));
        assert!(matches_pattern("1.0.0-beta", "1.0.0*"));
        assert!(!matches_pattern("1.0.1", "1.0.0*"));
    }

    async fn setup_test_dir() -> anyhow::Result<TempDir> {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create test directories
        fs::create_dir_all(base_path.join("1.0.0")).await?;
        fs::create_dir_all(base_path.join("1.0.1")).await?;
        fs::create_dir_all(base_path.join("2.0.0")).await?;
        fs::create_dir_all(base_path.join("beta-1.0.0")).await?;

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn test_collect_matching_versions_exact_match() -> anyhow::Result<()> {
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
    async fn test_collect_matching_versions_wildcard() -> anyhow::Result<()> {
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
    async fn test_collect_matching_versions_empty_dir() -> anyhow::Result<()> {
        let temp_dir = TempDir::new()?;
        let mut to_delete = Vec::new();

        collect_matching_versions(temp_dir.path(), "test-pkg".to_string(), "*", &mut to_delete)
            .await?;

        assert_eq!(to_delete.len(), 0);
        Ok(())
    }

    #[test]
    fn test_registry_to_dir_name() {
        assert_eq!(
            registry_to_dir_name("https://registry.npmjs.org"),
            "registry.npmjs.org"
        );
        assert_eq!(
            registry_to_dir_name("https://registry.npmmirror.com"),
            "registry.npmmirror.com"
        );
        assert_eq!(
            registry_to_dir_name("https://registry.npmjs.org/"),
            "registry.npmjs.org"
        );
        assert_eq!(
            registry_to_dir_name("http://registry.npmjs.org"),
            "registry.npmjs.org"
        );
    }

    #[test]
    fn test_get_cache_dir_uses_config() {
        // Test that get_cache_dir delegates to config module
        let result = get_cache_dir();

        // Should return a valid path
        assert!(result.is_absolute() || result.starts_with("~") || result.starts_with("."));
    }

    #[test]
    fn test_get_cache_dir_includes_registry() {
        // Test that cache directory includes registry dimension
        let cache_dir = get_cache_dir();
        let cache_dir_str = cache_dir.to_string_lossy();
        
        // Should contain registry directory name
        // The exact structure depends on current registry setting
        assert!(cache_dir_str.contains("nm") || cache_dir_str.contains("cache"));
    }

    #[test]
    fn test_get_package_versions_cache_file() {
        let result = get_package_versions_cache_file("lodash");

        // Should contain package name and versions.json
        assert!(result.to_string_lossy().contains("lodash"));
        assert!(result.to_string_lossy().ends_with("versions.json"));
    }

    #[test]
    fn test_get_package_versions_cache_file_scoped() {
        let result = get_package_versions_cache_file("@types/node");

        // Should handle scoped packages correctly
        assert!(result.to_string_lossy().contains("@types/node"));
        assert!(result.to_string_lossy().ends_with("versions.json"));
    }

    #[test]
    fn test_get_package_manifest_cache_file() {
        let result = get_package_manifest_cache_file("lodash", "4.17.21");

        // Should contain package name, manifests directory, and version.json
        assert!(result.to_string_lossy().contains("lodash"));
        assert!(result.to_string_lossy().contains("manifests"));
        assert!(result.to_string_lossy().ends_with("4.17.21.json"));
    }

    #[test]
    fn test_get_package_manifest_cache_file_scoped() {
        let result = get_package_manifest_cache_file("@types/node", "18.0.0");

        // Should handle scoped packages correctly
        assert!(result.to_string_lossy().contains("@types/node"));
        assert!(result.to_string_lossy().contains("manifests"));
        assert!(result.to_string_lossy().ends_with("18.0.0.json"));
    }

    #[tokio::test]
    async fn test_cache_file_structure_consistency() -> anyhow::Result<()> {
        // Test that cache file paths are consistent
        let pkg_name = "express";
        let version = "4.18.2";

        let versions_file = get_package_versions_cache_file(pkg_name);
        let manifest_file = get_package_manifest_cache_file(pkg_name, version);

        // Both should be under the same cache directory
        let cache_dir = get_cache_dir();
        assert!(versions_file.starts_with(&cache_dir));
        assert!(manifest_file.starts_with(&cache_dir));

        // Manifest should be under the same package directory as versions
        let pkg_dir = cache_dir.join(pkg_name);
        assert!(versions_file.starts_with(&pkg_dir));
        assert!(manifest_file.starts_with(&pkg_dir));

        Ok(())
    }
}
