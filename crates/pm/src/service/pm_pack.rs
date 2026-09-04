use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
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
    validate_pack_name(&pkg.name)?;

    // Normalize dependency protocols and publish-time manifest overrides.
    // `None` means there was nothing to rewrite, in which case the on-disk
    // package.json is packed verbatim.
    let normalized = normalize_publish_manifest(package_root, &pkg).await?;
    let pkg_json_override = normalized.as_ref().map(serialize_manifest).transpose()?;
    let packed_manifest = normalized.unwrap_or_else(|| pkg.clone());
    validate_pack_name(&packed_manifest.name)?;

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

/// Validate both the source and published package names using pnpm's
/// `validate-npm-package-name` `validForOldPackages` rules.
fn validate_pack_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Missing 'name' field in package.json");
    }
    if !is_valid_old_npm_package_name(name) {
        anyhow::bail!("Invalid package name \"{name}\".");
    }
    Ok(())
}

fn is_valid_old_npm_package_name(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('.')
        || name.starts_with('-')
        || name.starts_with('_')
        || name.trim() != name
        || name.eq_ignore_ascii_case("node_modules")
        || name.eq_ignore_ascii_case("favicon.ico")
    {
        return false;
    }
    if is_url_friendly_package_name_part(name) {
        return true;
    }

    let Some(rest) = name.strip_prefix('@') else {
        return false;
    };
    let Some((scope, package)) = rest.split_once('/') else {
        return false;
    };
    !scope.is_empty()
        && !package.is_empty()
        && !package.contains('/')
        && !package.starts_with('.')
        && is_url_friendly_package_name_part(scope)
        && is_url_friendly_package_name_part(package)
}

fn is_url_friendly_package_name_part(value: &str) -> bool {
    value.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
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
        builder.follow_symlinks(false);
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
    let mandatory_references = collect_mandatory_references(package_json);
    let root_ignore = if whitelist.is_none() {
        load_root_ignore(package_root)?
    } else {
        None
    };
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

    // Root ignore files can prune an entry point's parent directory before the
    // inclusion check sees it. Re-check only clean, exact main/browser/bin
    // references. Utoo deliberately does not emulate npm-packlist's ambiguous
    // slash and nested-ignore edge cases here.
    add_missing_referenced_files(
        package_root,
        root_ignore.as_ref(),
        &mandatory_references,
        &mut files,
    )?;

    files.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(files)
}

fn add_missing_referenced_files(
    package_root: &Path,
    root_ignore: Option<&Gitignore>,
    referenced_files: &std::collections::HashSet<String>,
    files: &mut Vec<(PathBuf, u64)>,
) -> Result<()> {
    let Some(root_ignore) = root_ignore else {
        return Ok(());
    };
    let canonical_root = fs::canonicalize(package_root)
        .with_context(|| format!("Failed to resolve {}", package_root.display()))?;
    let mut collected: std::collections::HashSet<PathBuf> =
        files.iter().map(|(path, _)| path.clone()).collect();
    for referenced in referenced_files {
        if referenced.ends_with('/') {
            continue;
        }
        let relative = PathBuf::from(referenced);
        if !root_ignore_prunes_reference(package_root, root_ignore, &relative) {
            continue;
        }
        if collected.contains(&relative) || !is_safe_referenced_path(&relative) {
            continue;
        }
        if let Some((actual_relative, size)) =
            resolve_referenced_file(package_root, &canonical_root, &relative)?
            && !contains_collected_path(&collected, &actual_relative)
        {
            collected.insert(actual_relative.clone());
            files.push((actual_relative, size));
        }
    }
    Ok(())
}

fn is_safe_referenced_path(path: &Path) -> bool {
    let mut has_component = false;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return false;
        };
        has_component = true;
        let name = name.to_string_lossy();
        if components.peek().is_some() && is_excluded_directory(&name) {
            return false;
        }
    }
    has_component
        && path
            .file_name()
            .is_some_and(|name| !is_excluded_file(&name.to_string_lossy()))
}

fn resolve_referenced_file(
    package_root: &Path,
    canonical_root: &Path,
    relative: &Path,
) -> Result<Option<(PathBuf, u64)>> {
    let components: Vec<_> = relative.components().collect();
    let mut current = package_root.to_path_buf();
    for component in &components {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to inspect {}", current.display()));
            }
        };
        if metadata.file_type().is_symlink() {
            return Ok(None);
        }
        if current != package_root.join(relative) && !metadata.is_dir() {
            return Ok(None);
        }
    }
    let canonical_file = fs::canonicalize(&current)
        .with_context(|| format!("Failed to resolve {}", current.display()))?;
    let Ok(actual_relative) = canonical_file.strip_prefix(canonical_root) else {
        return Ok(None);
    };
    if !is_safe_referenced_path(actual_relative) {
        return Ok(None);
    }
    let metadata = fs::metadata(&canonical_file)
        .with_context(|| format!("Failed to inspect {}", canonical_file.display()))?;
    Ok(metadata
        .is_file()
        .then_some((actual_relative.to_path_buf(), metadata.len())))
}

fn contains_collected_path(collected: &std::collections::HashSet<PathBuf>, path: &Path) -> bool {
    if cfg!(any(target_os = "macos", windows)) {
        let path = path.to_string_lossy();
        collected
            .iter()
            .any(|item| item.to_string_lossy().eq_ignore_ascii_case(&path))
    } else {
        collected.contains(path)
    }
}

fn root_ignore_prunes_reference(
    package_root: &Path,
    root_ignore: &Gitignore,
    relative: &Path,
) -> bool {
    if root_ignore
        .matched_path_or_any_parents(package_root.join(relative), false)
        .is_ignore()
    {
        return true;
    }
    let Some(parent) = relative.parent() else {
        return false;
    };
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        if root_ignore
            .matched(package_root.join(&current), true)
            .is_ignore()
        {
            return true;
        }
    }
    false
}

fn load_root_ignore(package_root: &Path) -> Result<Option<Gitignore>> {
    let npmignore = package_root.join(".npmignore");
    let ignore_file = if npmignore.exists() {
        npmignore
    } else {
        let gitignore = package_root.join(".gitignore");
        if !gitignore.exists() {
            return Ok(None);
        }
        gitignore
    };
    let mut builder = GitignoreBuilder::new(package_root);
    builder.case_insensitive(cfg!(any(target_os = "macos", windows)))?;
    if let Some(error) = builder.add(ignore_file) {
        return Err(error.into());
    }
    Ok(Some(builder.build()?))
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

fn collect_mandatory_references(pkg: &serde_json::Value) -> std::collections::HashSet<String> {
    let mut refs = std::collections::HashSet::new();
    for key in ["main", "browser"] {
        if let Some(reference) = pkg.get(key).and_then(|value| value.as_str())
            && let Some(reference) = clean_package_file_reference(reference)
        {
            refs.insert(reference);
        }
    }
    if let Some(bin) = pkg.get("bin") {
        match bin {
            serde_json::Value::String(reference) => {
                if let Some(reference) = clean_package_file_reference(reference) {
                    refs.insert(reference);
                }
            }
            serde_json::Value::Object(map) => {
                refs.extend(
                    map.values()
                        .filter_map(|value| value.as_str())
                        .filter_map(clean_package_file_reference),
                );
            }
            _ => {}
        }
    }
    refs
}

fn clean_package_file_reference(reference: &str) -> Option<String> {
    let normalized = reference.replace('\\', "/");
    let relative = normalized.strip_prefix("./").unwrap_or(&normalized);
    if relative.is_empty() || relative.starts_with('/') || relative.ends_with('/') {
        return None;
    }
    if relative.split('/').any(|component| {
        component.is_empty() || component == "." || component == ".." || component.contains(':')
    }) {
        return None;
    }
    Some(relative.to_string())
}

fn build_file_walker(package_root: &Path, has_whitelist: bool) -> WalkBuilder {
    let has_npmignore = package_root.join(".npmignore").exists();
    let mut builder = WalkBuilder::new(package_root);
    builder
        .hidden(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .git_ignore(false)
        .ignore_case_insensitive(cfg!(any(target_os = "macos", windows)));

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
            return !is_excluded_directory(&name);
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
    ALWAYS_EXCLUDE_FILES
        .iter()
        .any(|excluded| hard_name_eq(name, excluded))
        || ALWAYS_EXCLUDE_PREFIXES
            .iter()
            .any(|prefix| hard_name_starts_with(name, prefix))
        || ALWAYS_EXCLUDE_SUFFIXES
            .iter()
            .any(|suffix| hard_name_ends_with(name, suffix))
}

fn is_excluded_directory(name: &str) -> bool {
    ALWAYS_EXCLUDE_DIRS
        .iter()
        .any(|excluded| hard_name_eq(name, excluded))
}

fn hard_name_eq(name: &str, expected: &str) -> bool {
    if cfg!(any(target_os = "macos", windows)) {
        name.eq_ignore_ascii_case(expected)
    } else {
        name == expected
    }
}

fn hard_name_starts_with(name: &str, prefix: &str) -> bool {
    name.get(..prefix.len())
        .is_some_and(|start| hard_name_eq(start, prefix))
}

fn hard_name_ends_with(name: &str, suffix: &str) -> bool {
    name.get(name.len().saturating_sub(suffix.len())..)
        .is_some_and(|end| hard_name_eq(end, suffix))
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

    #[test]
    fn test_clean_package_file_reference() {
        for (input, expected) in [
            ("dist/index.js", Some("dist/index.js")),
            ("./dist/index.js", Some("dist/index.js")),
            (r"dist\index.js", Some("dist/index.js")),
            ("/dist/index.js", None),
            ("//dist/index.js", None),
            ("././dist/index.js", None),
            ("dist//index.js", None),
            ("dist/../index.js", None),
            ("C:/dist/index.js", None),
            ("dist/", None),
        ] {
            assert_eq!(
                clean_package_file_reference(input).as_deref(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn test_only_runtime_entrypoints_are_recovered_from_root_ignore() {
        for bin in [r#""dist/cli.js""#, r#"{ "fixture": "dist/cli.js" }"#] {
            let dir = TempDir::new().unwrap();
            let root = dir.path();
            let manifest = format!(
                r#"{{
  "name": "fixture",
  "version": "1.0.0",
  "main": "./dist/index.js",
  "browser": "dist/browser.js",
  "types": "dist/index.d.ts",
  "bin": {bin}
}}"#
            );
            fs::write(root.join("package.json"), &manifest).unwrap();
            fs::write(root.join(".npmignore"), "dist/\n").unwrap();
            fs::create_dir(root.join("dist")).unwrap();
            for file in ["index.js", "browser.js", "index.d.ts", "cli.js"] {
                fs::write(root.join("dist").join(file), file).unwrap();
            }

            let files = collect_pack_files(root, &parse_json(&manifest)).unwrap();
            assert!(has(&files, "dist/index.js"));
            assert!(has(&files, "dist/browser.js"));
            assert!(has(&files, "dist/cli.js"));
            assert!(!has(&files, "dist/index.d.ts"));
        }
    }

    #[test]
    fn test_nested_only_ignore_does_not_trigger_entrypoint_recovery() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let manifest = r#"{
  "name": "fixture",
  "version": "1.0.0",
  "browser": "dist/browser.js"
}"#;
        fs::write(root.join("package.json"), manifest).unwrap();
        fs::write(root.join(".npmignore"), "").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/.npmignore"), "browser.js\n").unwrap();
        fs::write(root.join("dist/browser.js"), "browser").unwrap();

        let files = collect_pack_files(root, &parse_json(manifest)).unwrap();
        assert!(!has(&files, "dist/browser.js"));
    }

    #[test]
    fn test_root_parent_ignore_with_entrypoint_negation_is_recovered() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let manifest = r#"{
  "name": "fixture",
  "version": "1.0.0",
  "browser": "dist/browser.js"
}"#;
        fs::write(root.join("package.json"), manifest).unwrap();
        fs::write(root.join(".npmignore"), "dist/\n!dist/browser.js\n").unwrap();
        fs::create_dir(root.join("dist")).unwrap();
        fs::write(root.join("dist/browser.js"), "browser").unwrap();

        let files = collect_pack_files(root, &parse_json(manifest)).unwrap();
        assert!(has(&files, "dist/browser.js"));
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn test_hard_exclusions_are_case_insensitive() {
        assert!(is_excluded_file(".NPMRC"));
        assert!(is_excluded_directory("NODE_MODULES"));

        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let manifest = r#"{
  "name": "fixture",
  "version": "1.0.0",
  "browser": ".NPMRC"
}"#;
        fs::write(root.join("package.json"), manifest).unwrap();
        fs::write(root.join(".npmignore"), "*\n").unwrap();
        fs::write(
            root.join(".npmrc"),
            "//registry.example/:_authToken=secret\n",
        )
        .unwrap();

        let files = collect_pack_files(root, &parse_json(manifest)).unwrap();
        assert!(!has(&files, ".NPMRC"));
        assert!(!has(&files, ".npmrc"));

        let alias_dir = TempDir::new().unwrap();
        let alias_root = alias_dir.path();
        let alias_manifest = r#"{
  "name": "fixture",
  "version": "1.0.0",
  "browser": "dist/browser.js"
}"#;
        fs::write(alias_root.join("package.json"), alias_manifest).unwrap();
        fs::write(alias_root.join(".npmignore"), "dist/browser.js\n").unwrap();
        fs::create_dir(alias_root.join("dist")).unwrap();
        fs::write(alias_root.join("dist/Browser.js"), "browser").unwrap();

        let files = collect_pack_files(alias_root, &parse_json(alias_manifest)).unwrap();
        assert_eq!(
            files
                .iter()
                .filter(|(path, _)| path
                    .to_string_lossy()
                    .eq_ignore_ascii_case("dist/browser.js"))
                .count(),
            1
        );
    }

    #[test]
    fn test_valid_old_npm_package_name() {
        for valid in [
            "foo",
            "foo-bar",
            "foo.bar",
            "foo_bar",
            "@scope/foo",
            "@scope/_foo",
            "Foo",
            "1.2.3",
        ] {
            assert!(
                is_valid_old_npm_package_name(valid),
                "{valid} should be valid"
            );
        }
        for invalid in [
            "",
            ".foo",
            "-foo",
            "_foo",
            " foo",
            "foo ",
            "bad name",
            "node_modules",
            "favicon.ico",
            "@scope/.foo",
            "@scope/foo/bar",
            "@/foo",
            "@scope/",
        ] {
            assert!(
                !is_valid_old_npm_package_name(invalid),
                "{invalid} should be invalid"
            );
        }
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
    fn test_referenced_file_cannot_escape_package_root() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("package");
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0","browser":"../secret.js"}"#,
        )
        .unwrap();
        fs::write(parent.path().join("secret.js"), "secret").unwrap();

        let data = parse_json(r#"{"name":"t","version":"1.0.0","browser":"../secret.js"}"#);
        let files = collect_pack_files(&root, &data).unwrap();
        assert!(!has(&files, "../secret.js"));
    }

    #[cfg(unix)]
    #[test]
    fn test_referenced_file_does_not_follow_symlinked_directory() {
        use std::os::unix::fs::symlink;

        let project = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let root = project.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"t","version":"1.0.0","browser":"dist/browser.js"}"#,
        )
        .unwrap();
        fs::write(root.join(".gitignore"), "dist/\n").unwrap();
        fs::write(outside.path().join("browser.js"), "secret").unwrap();
        symlink(outside.path(), root.join("dist")).unwrap();

        let data = parse_json(r#"{"name":"t","version":"1.0.0","browser":"dist/browser.js"}"#);
        let files = collect_pack_files(root, &data).unwrap();
        assert!(!has(&files, "dist/browser.js"));
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
