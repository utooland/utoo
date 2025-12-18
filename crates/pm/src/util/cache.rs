use tokio::fs;

pub use super::config::get_cache_dir;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

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
}
