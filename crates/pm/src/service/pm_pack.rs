use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::WalkBuilder;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use tar::{Builder, EntryType, Header};
use utoo_ruborist::manifest::PackageJson;

use crate::model::package::LifecycleHook;
use crate::model::package::PackageInfo;
use crate::service::publish_manifest::normalize_publish_manifest;
use crate::service::script::{ScriptOutput, ScriptService};
use crate::util::integrity::compute_integrity;
use crate::util::json::load_package_json;
use crate::util::user_config::get_or_load_package_json;

#[derive(Default)]
pub struct PackResult {
    pub tarball_data: Vec<u8>,
    pub files: Vec<(String, u64)>,
    pub name: String,
    pub version: String,
    pub integrity: String,
    pub unpacked_size: u64,
    pub packed_size: u64,
    /// The normalized manifest as written into the tarball. Reused to build the
    /// publish payload so registry metadata matches the packed `package.json`.
    pub manifest: PackageJson,
}

impl PackResult {
    /// Build the tarball filename from package name and version.
    ///
    /// Scoped packages have `@` and `/` stripped, e.g. `@scope/pkg` → `scope-pkg-1.0.0.tgz`.
    pub fn tarball_filename(&self) -> String {
        format!(
            "{}-{}.tgz",
            self.name.replace('/', "-").replace('@', ""),
            self.version
        )
    }
}

pub async fn pack(package_root: &Path, output: ScriptOutput) -> Result<PackResult> {
    let pkg = get_or_load_package_json(package_root).await?;
    let package_info = PackageInfo::from_package_json(package_root, &pkg)?;
    if pkg.version.is_empty() {
        anyhow::bail!("Missing 'version' field in package.json");
    }

    ScriptService::execute_script(&package_info, LifecycleHook::Prepack, output, None).await?;

    // npm/pnpm pack the post-`prepack` manifest, and the script may have
    // rewritten package.json (version bump, stripped fields). The `pkg` above
    // is cached from before the script ran, so read the current file from disk.
    let pkg: PackageJson = load_package_json(package_root).await?;

    // Normalize dependency protocols and publish-time manifest overrides.
    // `None` means there was nothing to rewrite, in which case the on-disk
    // package.json is packed verbatim.
    let normalized = normalize_publish_manifest(package_root, &pkg).await?;
    let pkg_json_override = normalized.as_ref().map(serialize_manifest).transpose()?;
    let packed_manifest = normalized.unwrap_or_else(|| pkg.clone());
    if packed_manifest.name.is_empty() {
        anyhow::bail!("Missing 'name' field in package.json");
    }

    // collect_pack_files uses ignore::WalkBuilder which does synchronous I/O.
    // Run on a blocking thread to avoid stalling the tokio runtime.
    let package_root_owned = package_root.to_path_buf();
    let collected = tokio::task::spawn_blocking({
        // Publish-time main/types/bin overrides affect npm's always-included
        // referenced files, so file selection must use the packed manifest.
        let data = packed_manifest.to_value();
        let package_root = package_root_owned.clone();
        move || collect_pack_files(&package_root, &data)
    })
    .await??;

    // When package.json is rewritten, account for the rewritten byte length
    // instead of the on-disk size in the reported stats.
    let effective_size = |path: &Path, on_disk: u64| match &pkg_json_override {
        Some(bytes) if is_root_manifest(path) => bytes.len() as u64,
        _ => on_disk,
    };
    let unpacked_size: u64 = collected
        .iter()
        .map(|(path, size)| effective_size(path, *size))
        .sum();
    let file_paths: Vec<(String, u64)> = collected
        .iter()
        .map(|(p, size)| (p.to_string_lossy().into_owned(), effective_size(p, *size)))
        .collect();

    // create_tarball reads each file via std::fs — also blocking I/O.
    let tar_data = tokio::task::spawn_blocking(move || {
        create_tarball(&package_root_owned, &collected, pkg_json_override)
    })
    .await??;
    let integrity = compute_integrity(&tar_data);
    let packed_size = tar_data.len() as u64;

    ScriptService::execute_script(&package_info, LifecycleHook::Postpack, output, None).await?;

    Ok(PackResult {
        tarball_data: tar_data,
        files: file_paths,
        name: packed_manifest.name.clone(),
        version: packed_manifest.version.clone(),
        integrity,
        unpacked_size,
        packed_size,
        manifest: packed_manifest,
    })
}

/// Whether `path` is the root-level `package.json` (the only manifest the
/// publish normalization substitutes — nested `package.json` files are packed
/// verbatim).
fn is_root_manifest(path: &Path) -> bool {
    path == Path::new("package.json")
}

/// Serialize a manifest to pretty 2-space JSON with a trailing newline, the
/// formatting npm writes into packed tarballs.
fn serialize_manifest(pkg: &PackageJson) -> Result<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec_pretty(pkg).context("Failed to serialize rewritten package.json")?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Create a gzip-compressed tarball from the collected file list.
///
/// Each file is archived under a `package/` prefix (npm tarball convention).
/// Pre-allocates the output buffer based on file sizes to reduce reallocation:
/// each tar entry has a 512-byte header, and compressed output is typically ~50% of raw size.
fn create_tarball(
    package_root: &Path,
    files: &[(PathBuf, u64)],
    pkg_json_override: Option<Vec<u8>>,
) -> Result<Vec<u8>> {
    let raw_estimate: usize = files.iter().map(|(_, size)| *size as usize + 512).sum();
    let mut encoder = GzEncoder::new(Vec::with_capacity(raw_estimate / 2), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);
        for (file_path, _) in files {
            let archive_path = Path::new("package").join(file_path);
            // Substitute the rewritten manifest for the on-disk package.json.
            if is_root_manifest(file_path)
                && let Some(bytes) = &pkg_json_override
            {
                append_override(&mut builder, package_root, &archive_path, bytes)?;
                continue;
            }
            let full_path = package_root.join(file_path);
            builder
                .append_path_with_name(&full_path, &archive_path)
                .with_context(|| format!("Failed to add {} to tarball", file_path.display()))?;
        }
        builder.finish()?;
    }
    Ok(encoder.finish()?)
}

/// Append in-memory bytes (the rewritten package.json) as a tarball entry,
/// preserving the original file's mode/mtime where available.
fn append_override<W: Write>(
    builder: &mut Builder<W>,
    package_root: &Path,
    archive_path: &Path,
    bytes: &[u8],
) -> Result<()> {
    let mut header = Header::new_gnu();
    match fs::metadata(package_root.join("package.json")) {
        Ok(meta) => header.set_metadata(&meta),
        // The manifest was just read, so a missing file here only means it was
        // racily removed — fall back to defaults. Surface any other error
        // (e.g. permissions) rather than masking it.
        Err(e) if e.kind() == ErrorKind::NotFound => {
            header.set_mode(0o644);
            header.set_mtime(0);
        }
        Err(e) => return Err(e).context("Failed to stat package.json for tarball header"),
    }
    header.set_entry_type(EntryType::file());
    header.set_size(bytes.len() as u64);
    header.set_cksum();
    builder
        .append_data(&mut header, archive_path, bytes)
        .context("Failed to add rewritten package.json to tarball")?;
    Ok(())
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
///    - `referenced_files`: paths declared in `main`, `browser`, `bin`, `types`, `typings` — always included
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
    for key in ["main", "browser", "types", "typings"] {
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

    #[test]
    fn test_tarball_filename_simple() {
        let r = PackResult {
            name: "my-pkg".into(),
            version: "1.0.0".into(),
            ..Default::default()
        };
        assert_eq!(r.tarball_filename(), "my-pkg-1.0.0.tgz");
    }

    #[test]
    fn test_tarball_filename_scoped() {
        let r = PackResult {
            name: "@scope/my-pkg".into(),
            version: "2.3.4".into(),
            ..Default::default()
        };
        assert_eq!(r.tarball_filename(), "scope-my-pkg-2.3.4.tgz");
    }

    #[test]
    fn test_tarball_filename_prerelease() {
        let r = PackResult {
            name: "pkg".into(),
            version: "1.0.0-beta.1".into(),
            ..Default::default()
        };
        assert_eq!(r.tarball_filename(), "pkg-1.0.0-beta.1.tgz");
    }
}
