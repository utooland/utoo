use std::path::PathBuf;
use tokio::fs;

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
        return (pattern.to_string(), "latest".to_string());
    }

    // no scope pkg
    let parts: Vec<&str> = pattern.rsplitn(2, '@').collect();
    match parts.as_slice() {
        [version, pkg] => (pkg.to_string(), version.to_string()),
        [pkg] => (pkg.to_string(), "latest".to_string()),
        _ => ("*".to_string(), "latest".to_string()),
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
) -> Result<(), std::io::Error> {
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

pub fn get_cache_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(".cache/nm")
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
            ("express".to_string(), "latest".to_string())
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
            ("@types/node".to_string(), "latest".to_string())
        );

        // scoped pkg with version
        assert_eq!(
            parse_pattern("@types/node@14.14.31"),
            ("@types/node".to_string(), "14.14.31".to_string())
        );

        // special case: @types/*
        assert_eq!(
            parse_pattern("@types/*"),
            ("@types/*".to_string(), "latest".to_string())
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

    async fn setup_test_dir() -> Result<TempDir, std::io::Error> {
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
    async fn test_collect_matching_versions_exact_match() -> Result<(), Box<dyn std::error::Error>>
    {
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
    async fn test_collect_matching_versions_wildcard() -> Result<(), Box<dyn std::error::Error>> {
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
    async fn test_collect_matching_versions_empty_dir() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let mut to_delete = Vec::new();

        collect_matching_versions(temp_dir.path(), "test-pkg".to_string(), "*", &mut to_delete)
            .await?;

        assert_eq!(to_delete.len(), 0);
        Ok(())
    }
}
