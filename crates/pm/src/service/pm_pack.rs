use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use tar::Builder;

use crate::model::package::PackageInfo;
use crate::service::script::ScriptService;
use crate::util::integrity::compute_integrity;
use crate::util::json::load_package_json_from_path;

pub struct PackResult {
    pub tarball_path: Option<PathBuf>,
    pub files: Vec<(String, u64)>,
    pub name: String,
    pub version: String,
    pub integrity: String,
    pub unpacked_size: u64,
    pub packed_size: u64,
}

pub async fn pack(package_root: &Path, dry_run: bool) -> Result<PackResult> {
    let data = load_package_json_from_path(package_root).await?;
    let package_info = PackageInfo::from_json(package_root, &data)?;
    let version = data["version"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'version' field in package.json"))?
        .to_string();

    ScriptService::execute_script(&package_info, "prepack", true).await?;

    // collect_pack_files uses ignore::WalkBuilder which does synchronous I/O.
    // Run on a blocking thread to avoid stalling the tokio runtime.
    let package_root_owned = package_root.to_path_buf();
    let collected = tokio::task::spawn_blocking({
        let data = data.clone();
        let package_root = package_root_owned.clone();
        move || collect_pack_files(&package_root, &data)
    })
    .await??;

    let unpacked_size: u64 = collected.iter().map(|(_, size)| size).sum();
    let file_paths: Vec<(String, u64)> = collected
        .iter()
        .map(|(p, size)| (p.to_string_lossy().to_string(), *size))
        .collect();

    if dry_run {
        return Ok(PackResult {
            tarball_path: None,
            files: file_paths,
            name: package_info.name,
            version,
            integrity: String::new(),
            unpacked_size,
            packed_size: 0,
        });
    }

    // create_tarball reads each file via std::fs — also blocking I/O.
    let tar_data =
        tokio::task::spawn_blocking(move || create_tarball(&package_root_owned, &collected))
            .await??;
    let integrity = compute_integrity(&tar_data);
    let packed_size = tar_data.len() as u64;

    let tarball_name = format!(
        "{}-{}.tgz",
        package_info.name.replace('/', "-").replace('@', ""),
        version
    );
    let tarball_path = package_root.join(&tarball_name);

    crate::fs::write(&tarball_path, &tar_data)
        .await
        .with_context(|| format!("Failed to write tarball to {}", tarball_path.display()))?;

    ScriptService::execute_script(&package_info, "postpack", true).await?;

    Ok(PackResult {
        tarball_path: Some(tarball_path),
        files: file_paths,
        name: package_info.name,
        version,
        integrity,
        unpacked_size,
        packed_size,
    })
}

/// Create a gzip-compressed tarball from the collected file list.
///
/// Each file is archived under a `package/` prefix (npm tarball convention).
/// Pre-allocates the output buffer based on file sizes to reduce reallocation:
/// each tar entry has a 512-byte header, and compressed output is typically ~50% of raw size.
fn create_tarball(package_root: &Path, files: &[(PathBuf, u64)]) -> Result<Vec<u8>> {
    let raw_estimate: usize = files.iter().map(|(_, size)| *size as usize + 512).sum();
    let mut encoder = GzEncoder::new(Vec::with_capacity(raw_estimate / 2), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);
        for (file_path, _) in files {
            let full_path = package_root.join(file_path);
            let archive_path = Path::new("package").join(file_path);
            builder
                .append_path_with_name(&full_path, &archive_path)
                .with_context(|| format!("Failed to add {} to tarball", file_path.display()))?;
        }
        builder.finish()?;
    }
    Ok(encoder.finish()?)
}

/// Collect files to include in a pack tarball, returning `(relative_path, size)` pairs.
///
/// Three-layer filtering pipeline (matches npm behavior):
///
/// 1. **Walker-level pruning** (`build_file_walker`):
///    - Entire directory trees like `node_modules`, `.git` are pruned via `filter_entry`
///    - Ignore files (`.npmignore` or `.gitignore`) are applied when no `files` whitelist exists;
///      in whitelist mode, ignore files are skipped entirely
///
/// 2. **Hard file exclusion** (`is_excluded_file`):
///    - Files that are never packed regardless of whitelist/ignore: `.DS_Store`, `.npmrc`,
///      `package-lock.json`, editor swap/backup files, etc.
///
/// 3. **Inclusion check** (determines whether a surviving file is collected):
///    - `is_always_included`: `package.json`, `readme*`, `license*` — always
///      included even if not listed in the `files` whitelist
///    - `referenced_files`: paths declared in `main`, `bin`, `types`, `typings` — always included
///    - Whitelist: if `files` field exists, the file must match a pattern; otherwise all files
///      that passed layers 1–2 are included
fn collect_pack_files(
    package_root: &Path,
    package_json: &serde_json::Value,
) -> Result<Vec<(PathBuf, u64)>> {
    let whitelist = compile_whitelist(package_json);
    let referenced_files = collect_referenced_files(package_json);
    let builder = build_file_walker(package_root, whitelist.is_some());

    let mut files = Vec::new();
    for result in builder.build() {
        let entry = result?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if is_excluded_file(&name) {
            continue;
        }
        let relative = entry.path().strip_prefix(package_root)?.to_path_buf();
        let rel_str = normalize_path(&relative.to_string_lossy());

        let included = is_always_included(&rel_str.to_lowercase())
            || referenced_files.contains(&rel_str)
            || match &whitelist {
                Some(wl) => matches_any(&rel_str, wl),
                None => true, // no whitelist = include all
            };

        if included {
            files.push((relative, entry.metadata()?.len()));
        }
    }

    files.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(files)
}

fn compile_whitelist(
    package_json: &serde_json::Value,
) -> Option<Vec<(String, Option<globset::GlobMatcher>)>> {
    package_json
        .get("files")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|pat| {
                    let norm = normalize_path(pat.trim_end_matches('/'));
                    let matcher = globset::Glob::new(&norm).ok().map(|g| g.compile_matcher());
                    (norm, matcher)
                })
                .collect()
        })
}

fn collect_referenced_files(pkg: &serde_json::Value) -> std::collections::HashSet<String> {
    let mut refs = std::collections::HashSet::new();
    for key in ["main", "types", "typings"] {
        if let Some(s) = pkg.get(key).and_then(|v| v.as_str()) {
            refs.insert(normalize_path(s));
        }
    }
    if let Some(bin) = pkg.get("bin") {
        match bin {
            serde_json::Value::String(s) => {
                refs.insert(normalize_path(s));
            }
            serde_json::Value::Object(map) => {
                refs.extend(map.values().filter_map(|v| v.as_str()).map(normalize_path));
            }
            _ => {}
        }
    }
    refs
}

fn build_file_walker(package_root: &Path, has_whitelist: bool) -> WalkBuilder {
    let has_npmignore = package_root.join(".npmignore").exists();
    let mut builder = WalkBuilder::new(package_root);
    builder
        .hidden(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .git_ignore(false);

    if has_whitelist {
        // Whitelist mode: no ignore files
    } else if has_npmignore {
        builder.add_custom_ignore_filename(".npmignore");
    } else {
        builder.add_custom_ignore_filename(".gitignore");
    }

    builder.filter_entry(|entry| {
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            let name = entry.file_name().to_string_lossy();
            return !ALWAYS_EXCLUDE_DIRS.contains(&name.as_ref());
        }
        true
    });

    builder
}

const ALWAYS_EXCLUDE_DIRS: &[&str] = &["node_modules", ".git", ".svn", ".hg", "CVS"];

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

const ALWAYS_EXCLUDE_PREFIXES: &[&str] = &["._", ".wafpickle-"];
const ALWAYS_EXCLUDE_SUFFIXES: &[&str] = &[".swp", ".orig"];

/// npm-packlist uses strict glob rules like `!/readme{,.*[^~$]}` to always include
/// certain files. The pattern matches the bare name or name + extension, but excludes
/// editor backup files ending in `~` or `$`. npm also includes `copying{,.*}`.
///
/// Our `starts_with` is more permissive (e.g. "readme-old-notes.txt" or "readme.md~"
/// would incorrectly match). This is a known simplification — tighten if needed.
fn is_always_included(lower_rel_path: &str) -> bool {
    // Only root-level files are always included
    if lower_rel_path.contains('/') {
        return false;
    }
    lower_rel_path == "package.json"
        || lower_rel_path.starts_with("readme")
        || lower_rel_path.starts_with("license")
        || lower_rel_path.starts_with("licence")
}

fn is_excluded_file(name: &str) -> bool {
    ALWAYS_EXCLUDE_FILES.contains(&name)
        || ALWAYS_EXCLUDE_PREFIXES.iter().any(|p| name.starts_with(p))
        || ALWAYS_EXCLUDE_SUFFIXES.iter().any(|s| name.ends_with(s))
}

fn matches_any(path: &str, compiled: &[(String, Option<globset::GlobMatcher>)]) -> bool {
    compiled.iter().any(|(pat, m)| {
        path == pat
            || path.starts_with(&format!("{pat}/"))
            || m.as_ref().is_some_and(|m| m.is_match(path))
    })
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

#[cfg(test)]
fn matches_glob(path: &str, pattern: &str) -> bool {
    let pattern = normalize_path(pattern.trim_end_matches('/'));

    if path == pattern || path.starts_with(&format!("{pattern}/")) {
        return true;
    }

    globset::Glob::new(&pattern)
        .map(|g| g.compile_matcher().is_match(path))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_matches_glob() {
        assert!(matches_glob("src/index.js", "src"));
        assert!(matches_glob("dist/bundle.js", "dist"));
        assert!(!matches_glob("src/index.js", "dist"));
        assert!(matches_glob("src/index.js", "src/*"));
        assert!(matches_glob("index.js", "*.js"));
        assert!(matches_glob("src/foo/bar.test.js", "**/*.test.js"));
        assert!(matches_glob("bar.test.js", "**/*.test.js"));
        assert!(!matches_glob("bar.test.ts", "**/*.test.js"));
        assert!(matches_glob("a/b/c", "a/**/c"));
        assert!(matches_glob("a/c", "a/**/c"));
        assert!(matches_glob("a/x/y/c", "a/**/c"));
    }

    fn parse_json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    fn has(files: &[(PathBuf, u64)], name: &str) -> bool {
        files.iter().any(|(p, _)| p == Path::new(name))
    }

    #[test]
    fn test_collect_basic() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join("index.js"), "module.exports = {}").unwrap();
        fs::write(root.join("README.md"), "# Test").unwrap();

        let files =
            collect_pack_files(root, &parse_json(r#"{"name":"t","version":"1.0.0"}"#)).unwrap();
        assert!(has(&files, "package.json"));
        assert!(has(&files, "README.md"));
        assert!(has(&files, "index.js"));
    }

    #[test]
    fn test_collect_whitelist() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0","files":["dist"]}"#,
        )
        .unwrap();
        fs::write(root.join("index.js"), "// source").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/bundle.js"), "// bundle").unwrap();
        fs::write(root.join("README.md"), "# Test").unwrap();

        let data = parse_json(r#"{"name":"t","version":"1.0.0","files":["dist"]}"#);
        let files = collect_pack_files(root, &data).unwrap();
        assert!(has(&files, "package.json"));
        assert!(has(&files, "README.md"));
        assert!(has(&files, "dist/bundle.js"));
        assert!(!has(&files, "index.js"));
    }

    #[test]
    fn test_collect_npmignore() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join(".npmignore"), "src\n*.test.js\n").unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();
        fs::write(root.join("foo.test.js"), "// test").unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.js"), "// lib").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/bundle.js"), "// bundle").unwrap();

        let files =
            collect_pack_files(root, &parse_json(r#"{"name":"t","version":"1.0.0"}"#)).unwrap();
        assert!(has(&files, "index.js"));
        assert!(has(&files, "dist/bundle.js"));
        assert!(!has(&files, "src/lib.js"));
        assert!(!has(&files, "foo.test.js"));
    }

    #[test]
    fn test_collect_gitignore_fallback() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join(".gitignore"), "dist\n").unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/bundle.js"), "// bundle").unwrap();

        let files =
            collect_pack_files(root, &parse_json(r#"{"name":"t","version":"1.0.0"}"#)).unwrap();
        assert!(has(&files, "index.js"));
        assert!(!has(&files, "dist/bundle.js"));
    }

    #[test]
    fn test_collect_negation() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join(".npmignore"), "*.log\n!important.log\n").unwrap();
        fs::write(root.join("debug.log"), "debug").unwrap();
        fs::write(root.join("important.log"), "important").unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();

        let files =
            collect_pack_files(root, &parse_json(r#"{"name":"t","version":"1.0.0"}"#)).unwrap();
        assert!(has(&files, "index.js"));
        assert!(has(&files, "important.log"));
        assert!(!has(&files, "debug.log"));
    }

    #[test]
    fn test_always_excluded_files_not_packed() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(root.join("index.js"), "// main").unwrap();
        fs::write(root.join(".DS_Store"), "").unwrap();
        fs::write(root.join(".npmrc"), "//token").unwrap();
        fs::write(root.join("package-lock.json"), "{}").unwrap();
        fs::write(root.join("._metadata"), "").unwrap();

        let files =
            collect_pack_files(root, &parse_json(r#"{"name":"t","version":"1.0.0"}"#)).unwrap();
        assert!(has(&files, "index.js"));
        for excluded in [".DS_Store", ".npmrc", "package-lock.json", "._metadata"] {
            assert!(!has(&files, excluded), "{excluded} should be excluded");
        }
    }
}
