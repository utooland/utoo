pub use super::user_config::get_cache_dir;

/// Glob-style match supporting `*` (any), `prefix*`, `*suffix`, `a*b`, and
/// `@scope/*`. Shared by cache-clean version filtering and workspace filters.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
