use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct PackfileConfig {
    /// Relative paths from package root
    pub files: Vec<PathBuf>,
    pub total_size: u64,
}

/// Directories always excluded from pack (never walked into).
const ALWAYS_EXCLUDE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "CVS",
];

/// Files always excluded from pack, regardless of config.
const ALWAYS_EXCLUDE_FILES: &[&str] = &[
    ".npmrc",
    ".DS_Store",
    ".gitignore",
    ".npmignore",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "npm-debug.log",
    ".lock-wscript",
    ".wafpickle-0",
];

/// File prefixes always excluded.
const ALWAYS_EXCLUDE_PREFIXES: &[&str] = &["._", ".wafpickle-"];

/// File suffixes always excluded.
const ALWAYS_EXCLUDE_SUFFIXES: &[&str] = &[".swp", ".orig"];

/// Collect files to include in a pack tarball.
///
/// Rules (matching npm behavior):
/// - Always include: package.json, README*, LICENSE*, LICENCE*, CHANGELOG*
/// - Always exclude: node_modules/, .git/, .npmrc, .DS_Store, lock files, etc.
/// - If package.json "files" field exists → whitelist mode (only listed files + always-included)
/// - If no "files" field:
///   - If .npmignore exists → use it for exclusion (ignore .gitignore)
///   - Else if .gitignore exists → use it as fallback
///   - Else → include everything (except always-excluded)
pub async fn collect_pack_files(package_root: &Path) -> Result<PackfileConfig> {
    let package_json_str = crate::fs::read_to_string(&package_root.join("package.json"))
        .await
        .context("Failed to read package.json")?;
    let data: serde_json::Value =
        serde_json::from_str(&package_json_str).context("Failed to parse package.json")?;

    // Check for "files" field in package.json
    let files_whitelist: Option<Vec<String>> = data
        .get("files")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });

    // Collect ignore rules if no whitelist
    let ignore_rules = if files_whitelist.is_none() {
        load_ignore_rules(package_root)?
    } else {
        vec![]
    };

    // Collect referenced files (main, bin, types, etc.)
    let mut referenced_files = Vec::new();
    if let Some(main) = data.get("main").and_then(|m| m.as_str()) {
        referenced_files.push(normalize_path(main));
    }
    if let Some(bin) = data.get("bin") {
        collect_bin_paths(bin, &mut referenced_files);
    }
    if let Some(types) = data
        .get("types")
        .or_else(|| data.get("typings"))
        .and_then(|t| t.as_str())
    {
        referenced_files.push(normalize_path(types));
    }

    let mut files = Vec::new();
    let mut total_size = 0u64;

    walk_dir(
        package_root,
        package_root,
        &files_whitelist,
        &ignore_rules,
        &referenced_files,
        &mut files,
        &mut total_size,
    )
    .await?;

    files.sort();
    Ok(PackfileConfig { files, total_size })
}

fn walk_dir<'a>(
    root: &'a Path,
    dir: &'a Path,
    whitelist: &'a Option<Vec<String>>,
    ignore_rules: &'a [IgnoreRule],
    referenced_files: &'a [String],
    files: &'a mut Vec<PathBuf>,
    total_size: &'a mut u64,
) -> futures::future::BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let mut entries = crate::fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            if path.is_dir() {
                // Always-excluded directories
                if ALWAYS_EXCLUDE_DIRS.contains(&name.as_ref()) {
                    continue;
                }

                // Check ignore rules against directory path
                let relative = path
                    .strip_prefix(root)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| path.clone());
                let rel_str = normalize_path(&relative.to_string_lossy());

                if whitelist.is_none() && is_ignored(&rel_str, true, ignore_rules) {
                    continue;
                }

                walk_dir(
                    root,
                    &path,
                    whitelist,
                    ignore_rules,
                    referenced_files,
                    files,
                    total_size,
                )
                .await?;
            } else {
                if is_always_excluded_file(&name) {
                    continue;
                }

                let relative = path
                    .strip_prefix(root)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|_| path.clone());
                let rel_str = normalize_path(&relative.to_string_lossy());

                if should_include(
                    &rel_str,
                    &name,
                    whitelist,
                    ignore_rules,
                    referenced_files,
                ) {
                    let meta = crate::fs::metadata(&path).await?;
                    *total_size += meta.len();
                    files.push(relative);
                }
            }
        }

        Ok(())
    })
}

fn should_include(
    relative_path: &str,
    file_name: &str,
    whitelist: &Option<Vec<String>>,
    ignore_rules: &[IgnoreRule],
    referenced_files: &[String],
) -> bool {
    // Always include certain files
    if is_always_included(file_name) {
        return true;
    }

    // Always include referenced files (main, bin, types)
    let normalized = normalize_path(relative_path);
    for rf in referenced_files {
        if normalized == *rf {
            return true;
        }
    }

    // Whitelist mode
    if let Some(wl) = whitelist {
        return wl.iter().any(|pattern| matches_glob(&normalized, pattern));
    }

    // Ignore mode: evaluate rules in order, last match wins (gitignore semantics)
    !is_ignored(&normalized, false, ignore_rules)
}

/// Check if a path is ignored by the rules. Last matching rule wins (gitignore semantics).
fn is_ignored(path: &str, is_dir: bool, rules: &[IgnoreRule]) -> bool {
    let mut ignored = false;
    for rule in rules {
        // Directory-only rules only apply to directories
        if rule.dir_only && !is_dir {
            continue;
        }
        if rule.matches(path, is_dir) {
            ignored = !rule.negated;
        }
    }
    ignored
}

fn is_always_included(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    lower == "package.json"
        || lower.starts_with("readme")
        || lower.starts_with("license")
        || lower.starts_with("licence")
        || lower.starts_with("changelog")
}

fn is_always_excluded_file(file_name: &str) -> bool {
    if ALWAYS_EXCLUDE_FILES.contains(&file_name) {
        return true;
    }
    for prefix in ALWAYS_EXCLUDE_PREFIXES {
        if file_name.starts_with(prefix) {
            return true;
        }
    }
    for suffix in ALWAYS_EXCLUDE_SUFFIXES {
        if file_name.ends_with(suffix) {
            return true;
        }
    }
    false
}

pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

/// A parsed ignore rule with gitignore semantics.
#[derive(Debug, Clone)]
struct IgnoreRule {
    /// The glob pattern to match.
    pattern: String,
    /// Whether this rule is negated (re-includes files).
    negated: bool,
    /// Whether this rule only applies to directories (trailing /).
    dir_only: bool,
    /// Whether this rule is anchored to root (contains / other than trailing).
    anchored: bool,
}

impl IgnoreRule {
    fn parse(line: &str) -> Option<Self> {
        let mut s = line.trim().to_string();

        if s.is_empty() || s.starts_with('#') {
            return None;
        }

        let negated = s.starts_with('!');
        if negated {
            s = s[1..].to_string();
        }

        // Trim leading/trailing whitespace after negation removal
        s = s.trim().to_string();
        if s.is_empty() {
            return None;
        }

        let dir_only = s.ends_with('/');
        if dir_only {
            s = s.trim_end_matches('/').to_string();
        }

        // Remove leading / (makes it anchored to root)
        let anchored = s.contains('/');
        s = s.trim_start_matches('/').to_string();

        Some(IgnoreRule {
            pattern: s,
            negated,
            dir_only,
            anchored,
        })
    }

    fn matches(&self, path: &str, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }

        if self.anchored {
            // Anchored: match from root
            self.glob_match(path)
        } else {
            // Unanchored: match against basename OR full path
            // e.g. "*.log" matches "foo/bar.log" and "bar.log"
            if self.glob_match(path) {
                return true;
            }
            // Try matching against just the filename
            if let Some(basename) = path.rsplit('/').next() {
                self.glob_match(basename)
            } else {
                false
            }
        }
    }

    fn glob_match(&self, path: &str) -> bool {
        let pattern = &self.pattern;

        // Handle ** patterns
        if pattern.contains("**") {
            return self.glob_match_doublestar(path);
        }

        // Simple glob via glob crate
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            glob::Pattern::new(pattern)
                .map(|p| p.matches(path))
                .unwrap_or(false)
        } else {
            // Exact match or directory prefix match
            path == *pattern || path.starts_with(&format!("{}/", pattern))
        }
    }

    fn glob_match_doublestar(&self, path: &str) -> bool {
        let pattern = &self.pattern;

        // Convert gitignore ** to glob ** rules:
        // "**/*.js" → match any .js file at any depth
        // "a/**/b" → match a/b, a/x/b, a/x/y/b
        // "**" → match everything

        // Split both pattern and path by /
        let pat_parts: Vec<&str> = pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        doublestar_match(&pat_parts, &path_parts)
    }
}

/// Recursive doublestar matching.
fn doublestar_match(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }

    if pattern[0] == "**" {
        // ** matches zero or more path segments
        let rest_pattern = &pattern[1..];
        // Try matching rest_pattern against every suffix of path
        for i in 0..=path.len() {
            if doublestar_match(rest_pattern, &path[i..]) {
                return true;
            }
        }
        return false;
    }

    if path.is_empty() {
        return false;
    }

    // Match current segment with glob
    let seg_pattern = pattern[0];
    if seg_pattern.contains('*') || seg_pattern.contains('?') || seg_pattern.contains('[') {
        if let Ok(p) = glob::Pattern::new(seg_pattern) {
            if p.matches(path[0]) {
                return doublestar_match(&pattern[1..], &path[1..]);
            }
        }
        false
    } else if seg_pattern == path[0] {
        doublestar_match(&pattern[1..], &path[1..])
    } else {
        false
    }
}

fn collect_bin_paths(bin: &serde_json::Value, paths: &mut Vec<String>) {
    match bin {
        serde_json::Value::String(s) => paths.push(normalize_path(s)),
        serde_json::Value::Object(map) => {
            for value in map.values() {
                if let Some(s) = value.as_str() {
                    paths.push(normalize_path(s));
                }
            }
        }
        _ => {}
    }
}

fn load_ignore_rules(root: &Path) -> Result<Vec<IgnoreRule>> {
    // .npmignore takes precedence; .gitignore is fallback
    let npmignore = root.join(".npmignore");
    let gitignore = root.join(".gitignore");

    let path = if npmignore.exists() {
        Some(npmignore)
    } else if gitignore.exists() {
        Some(gitignore)
    } else {
        None
    };

    match path {
        Some(p) => parse_ignore_file(&p),
        None => Ok(vec![]),
    }
}

fn parse_ignore_file(path: &Path) -> Result<Vec<IgnoreRule>> {
    let content = std::fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter_map(IgnoreRule::parse)
        .collect())
}

// Keep public for backward compat with existing test references
pub fn matches_glob(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    let pattern = normalize_path(pattern);

    if pattern.contains("**") {
        let pat_parts: Vec<&str> = pattern.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();
        return doublestar_match(&pat_parts, &path_parts);
    }

    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        glob::Pattern::new(&pattern)
            .map(|p| p.matches(path))
            .unwrap_or(false)
    } else {
        // Exact match or directory prefix match
        path == pattern || path.starts_with(&format!("{pattern}/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_always_included() {
        assert!(is_always_included("package.json"));
        assert!(is_always_included("README.md"));
        assert!(is_always_included("readme"));
        assert!(is_always_included("LICENSE"));
        assert!(is_always_included("LICENCE"));
        assert!(is_always_included("CHANGELOG.md"));
        assert!(!is_always_included("index.js"));
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("./src/index.js"), "src/index.js");
        assert_eq!(normalize_path("src\\lib.rs"), "src/lib.rs");
        assert_eq!(normalize_path("dist/index.js"), "dist/index.js");
    }

    #[test]
    fn test_matches_glob() {
        assert!(matches_glob("src/index.js", "src"));
        assert!(matches_glob("src/index.js", "src/*"));
        assert!(matches_glob("dist/bundle.js", "dist"));
        assert!(!matches_glob("src/index.js", "dist"));
        assert!(matches_glob("index.js", "*.js"));
    }

    #[test]
    fn test_matches_glob_doublestar() {
        assert!(matches_glob("src/foo/bar.test.js", "**/*.test.js"));
        assert!(matches_glob("bar.test.js", "**/*.test.js"));
        assert!(!matches_glob("bar.test.ts", "**/*.test.js"));
        assert!(matches_glob("a/b/c", "a/**/c"));
        assert!(matches_glob("a/c", "a/**/c"));
        assert!(matches_glob("a/x/y/c", "a/**/c"));
    }

    #[test]
    fn test_parse_ignore_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".npmignore");
        fs::write(&path, "node_modules\n# comment\n\nsrc\n!important\n").unwrap();

        let rules = parse_ignore_file(&path).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].pattern, "node_modules");
        assert!(!rules[0].negated);
        assert_eq!(rules[1].pattern, "src");
        assert!(!rules[1].negated);
        assert_eq!(rules[2].pattern, "important");
        assert!(rules[2].negated);
    }

    #[test]
    fn test_ignore_rule_parse() {
        // Basic pattern
        let rule = IgnoreRule::parse("*.log").unwrap();
        assert_eq!(rule.pattern, "*.log");
        assert!(!rule.negated);
        assert!(!rule.dir_only);

        // Negation
        let rule = IgnoreRule::parse("!important.log").unwrap();
        assert_eq!(rule.pattern, "important.log");
        assert!(rule.negated);

        // Directory only
        let rule = IgnoreRule::parse("build/").unwrap();
        assert_eq!(rule.pattern, "build");
        assert!(rule.dir_only);

        // Anchored (contains /)
        let rule = IgnoreRule::parse("src/test").unwrap();
        assert!(rule.anchored);

        // Comment
        assert!(IgnoreRule::parse("# comment").is_none());

        // Empty
        assert!(IgnoreRule::parse("").is_none());
    }

    #[test]
    fn test_ignore_rule_matching() {
        // Unanchored: *.log matches at any depth
        let rule = IgnoreRule::parse("*.log").unwrap();
        assert!(rule.matches("debug.log", false));
        assert!(rule.matches("src/debug.log", false));
        assert!(rule.matches("a/b/c/debug.log", false));
        assert!(!rule.matches("debug.txt", false));

        // Anchored: src/test only matches from root
        let rule = IgnoreRule::parse("src/test").unwrap();
        assert!(rule.matches("src/test", false));
        assert!(rule.matches("src/test/foo.js", false));
        assert!(!rule.matches("other/src/test", false));

        // Directory only
        let rule = IgnoreRule::parse("build/").unwrap();
        assert!(rule.matches("build", true));
        assert!(!rule.matches("build", false)); // not a directory
        assert!(!rule.matches("build.js", false));

        // Doublestar
        let rule = IgnoreRule::parse("**/*.test.js").unwrap();
        assert!(rule.matches("foo.test.js", false));
        assert!(rule.matches("src/foo.test.js", false));
        assert!(rule.matches("src/deep/foo.test.js", false));
    }

    #[test]
    fn test_is_ignored_last_match_wins() {
        let rules = vec![
            IgnoreRule::parse("*.log").unwrap(),
            IgnoreRule::parse("!important.log").unwrap(),
        ];

        // *.log ignores all .log files, but !important.log re-includes it
        assert!(is_ignored("debug.log", false, &rules));
        assert!(!is_ignored("important.log", false, &rules));
    }

    #[test]
    fn test_is_always_excluded_file() {
        assert!(is_always_excluded_file(".DS_Store"));
        assert!(is_always_excluded_file(".npmrc"));
        assert!(is_always_excluded_file(".gitignore"));
        assert!(is_always_excluded_file(".npmignore"));
        assert!(is_always_excluded_file("package-lock.json"));
        assert!(is_always_excluded_file("yarn.lock"));
        assert!(is_always_excluded_file("._metadata"));
        assert!(is_always_excluded_file("test.swp"));
        assert!(!is_always_excluded_file("index.js"));
    }

    #[tokio::test]
    async fn test_collect_pack_files_basic() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create package.json
        fs::write(
            root.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();

        // Create some files
        fs::write(root.join("index.js"), "module.exports = {}").unwrap();
        fs::write(root.join("README.md"), "# Test").unwrap();

        let config = collect_pack_files(root).await.unwrap();
        assert!(config.files.iter().any(|f| f == Path::new("package.json")));
        assert!(config.files.iter().any(|f| f == Path::new("README.md")));
        assert!(config.files.iter().any(|f| f == Path::new("index.js")));
    }

    #[tokio::test]
    async fn test_collect_pack_files_whitelist() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create package.json with files field
        fs::write(
            root.join("package.json"),
            r#"{"name":"test","version":"1.0.0","files":["dist"]}"#,
        )
        .unwrap();

        // Create files
        fs::write(root.join("index.js"), "// source").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/bundle.js"), "// bundle").unwrap();
        fs::write(root.join("README.md"), "# Test").unwrap();

        let config = collect_pack_files(root).await.unwrap();
        // package.json and README.md always included
        assert!(config.files.iter().any(|f| f == Path::new("package.json")));
        assert!(config.files.iter().any(|f| f == Path::new("README.md")));
        // dist/bundle.js included via whitelist
        assert!(config
            .files
            .iter()
            .any(|f| f == Path::new("dist/bundle.js")));
        // index.js NOT included (not in whitelist)
        assert!(!config.files.iter().any(|f| f == Path::new("index.js")));
    }

    #[tokio::test]
    async fn test_collect_pack_files_npmignore() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join(".npmignore"), "src\n*.test.js\n").unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();
        fs::write(root.join("foo.test.js"), "// test").unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.js"), "// lib").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/bundle.js"), "// bundle").unwrap();

        let config = collect_pack_files(root).await.unwrap();
        assert!(config.files.iter().any(|f| f == Path::new("index.js")));
        assert!(config
            .files
            .iter()
            .any(|f| f == Path::new("dist/bundle.js")));
        // Excluded by .npmignore
        assert!(!config.files.iter().any(|f| f == Path::new("src/lib.js")));
        assert!(!config
            .files
            .iter()
            .any(|f| f == Path::new("foo.test.js")));
    }

    #[tokio::test]
    async fn test_collect_pack_files_gitignore_fallback() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();
        // No .npmignore, should fall back to .gitignore
        fs::write(root.join(".gitignore"), "dist\n").unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/bundle.js"), "// bundle").unwrap();

        let config = collect_pack_files(root).await.unwrap();
        assert!(config.files.iter().any(|f| f == Path::new("index.js")));
        // Excluded by .gitignore
        assert!(!config
            .files
            .iter()
            .any(|f| f == Path::new("dist/bundle.js")));
    }

    #[tokio::test]
    async fn test_collect_pack_files_negation() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();
        // Ignore all .log files except important.log
        fs::write(root.join(".npmignore"), "*.log\n!important.log\n").unwrap();
        fs::write(root.join("debug.log"), "debug").unwrap();
        fs::write(root.join("important.log"), "important").unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();

        let config = collect_pack_files(root).await.unwrap();
        assert!(config.files.iter().any(|f| f == Path::new("index.js")));
        assert!(config
            .files
            .iter()
            .any(|f| f == Path::new("important.log")));
        // Excluded
        assert!(!config.files.iter().any(|f| f == Path::new("debug.log")));
    }

    #[tokio::test]
    async fn test_always_excluded_files_not_packed() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(
            root.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();
        fs::write(root.join(".DS_Store"), "").unwrap();
        fs::write(root.join(".npmrc"), "//token").unwrap();
        fs::write(root.join("package-lock.json"), "{}").unwrap();
        fs::write(root.join("._metadata"), "").unwrap();

        let config = collect_pack_files(root).await.unwrap();
        assert!(config.files.iter().any(|f| f == Path::new("index.js")));
        assert!(!config.files.iter().any(|f| f == Path::new(".DS_Store")));
        assert!(!config.files.iter().any(|f| f == Path::new(".npmrc")));
        assert!(!config
            .files
            .iter()
            .any(|f| f == Path::new("package-lock.json")));
        assert!(!config
            .files
            .iter()
            .any(|f| f == Path::new("._metadata")));
    }
}
