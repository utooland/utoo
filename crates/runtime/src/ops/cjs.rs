use std::cell::RefCell;
use std::path::{Path, PathBuf};

use deno_core::op2;
use deno_error::JsErrorBox;

use crate::transpile::transpile_to_js;

// Track the current CJS file being executed (for dynamic import() resolution)
thread_local! {
    pub static CURRENT_CJS_FILE: RefCell<String> = const { RefCell::new(String::new()) };
}

// ---------------------------------------------------------------------------
// Ops
// ---------------------------------------------------------------------------

/// Resolve a CJS specifier from a referrer file path.
/// Returns an absolute file path or "node:<name>" for built-ins.
#[op2]
#[string]
pub fn op_cjs_resolve(
    #[string] specifier: &str,
    #[string] referrer: &str,
) -> Result<String, JsErrorBox> {
    // Track current CJS file for dynamic import() resolution
    if !referrer.is_empty() {
        CURRENT_CJS_FILE.with(|f| *f.borrow_mut() = referrer.to_string());
    }

    // 1. Built-in?
    let bare = specifier.strip_prefix("node:").unwrap_or(specifier);
    if is_node_builtin(bare) {
        return Ok(format!("node:{bare}"));
    }

    let referrer_dir = if referrer.is_empty() {
        std::env::current_dir().map_err(|e| JsErrorBox::generic(e.to_string()))?
    } else {
        // Resolve symlinks so that require('..') from a symlinked bin script
        // resolves relative to the real file location, not the symlink location.
        let real_referrer = std::fs::canonicalize(referrer)
            .unwrap_or_else(|_| PathBuf::from(referrer));
        real_referrer
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf()
    };

    // 2. Relative / absolute
    if specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier == "." || specifier == ".."
        || specifier.starts_with('/')
    {
        let resolved = referrer_dir.join(specifier);
        return resolve_as_file_or_directory(&resolved)
            .and_then(|p| normalize_path(&p))
            .map(|p| p.to_string_lossy().into_owned())
            .ok_or_else(|| {
                JsErrorBox::generic(format!(
                    "Cannot find module '{specifier}' from '{referrer}'"
                ))
            });
    }

    // 3. node_modules walk (CJS require context)
    resolve_from_node_modules_dir(specifier, &referrer_dir, false)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| JsErrorBox::generic(e))
}

/// Detect whether a file should be treated as CommonJS.
#[op2(fast)]
pub fn op_cjs_detect(#[string] path: &str) -> bool {
    is_cjs(Path::new(path))
}

/// Transpile TypeScript source to JavaScript (for require of .ts files).
#[op2]
#[string]
pub fn op_cjs_transpile(
    #[string] source: &str,
    #[string] filename: &str,
) -> Result<String, JsErrorBox> {
    transpile_to_js(source, Path::new(filename))
        .map_err(|e| JsErrorBox::generic(e.to_string()))
}

// ---------------------------------------------------------------------------
// Public helpers (used by loader.rs)
// ---------------------------------------------------------------------------

pub fn is_node_builtin(name: &str) -> bool {
    let name = name.strip_prefix("node:").unwrap_or(name);
    matches!(
        name,
        "fs" | "fs/promises"
            | "path"
            | "os"
            | "url"
            | "buffer"
            | "assert"
            | "util"
            | "events"
            | "stream"
            | "http"
            | "https"
            | "http2"
            | "crypto"
            | "child_process"
            | "net"
            | "tls"
            | "querystring"
            | "string_decoder"
            | "module"
            | "process"
            | "async_hooks"
            | "zlib"
            | "v8"
            | "cluster"
            | "tty"
            | "dns"
            | "dgram"
            | "worker_threads"
            | "perf_hooks"
            | "readline"
            | "diagnostics_channel"
            | "console"
            | "timers"
            | "timers/promises"
            | "constants"
            | "domain"
            | "util/types"
            | "stream/promises"
            | "stream/web"
            | "stream/consumers"
            | "inspector"
            | "dns/promises"
            | "path/posix"
            | "vm"
            | "repl"
    )
}

/// Determine if a file path should be loaded as CJS.
pub fn is_cjs(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some("cjs") => true,
        Some("mjs" | "mts") => false,
        Some("js" | "ts" | "tsx" | "jsx") => {
            // Walk up to find nearest package.json with "type" field
            let mut dir = path.parent();
            while let Some(d) = dir {
                let pkg = d.join("package.json");
                if pkg.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&pkg) {
                        if let Ok(json) =
                            serde_json::from_str::<serde_json::Value>(&content)
                        {
                            if json.get("type").and_then(|v| v.as_str())
                                == Some("module")
                            {
                                return false;
                            }
                            return true;
                        }
                    }
                    return true;
                }
                dir = d.parent();
            }
            true // Default: no package.json found → CJS
        }
        _ => false,
    }
}

/// Resolve a bare specifier from node_modules, walking up from `from_dir`.
/// `esm` selects the active export conditions: ESM imports prefer `import`,
/// CJS requires prefer `require` (both fall back to `node`/`default`). This is
/// essential for dual packages (e.g. `utility`) whose named ESM exports are
/// only present in the `import` build.
pub fn resolve_from_node_modules_dir(
    specifier: &str,
    from_dir: &Path,
    esm: bool,
) -> Result<PathBuf, String> {
    let (pkg_name, subpath) = parse_package_name(specifier);

    let mut dir = Some(from_dir);
    while let Some(d) = dir {
        let nm = d.join("node_modules").join(pkg_name);
        if nm.is_dir() {
            let target = if subpath.is_empty() {
                // Try exports["."] first, then main/index
                if let Some(resolved) = resolve_package_exports(&nm, ".", esm) {
                    resolved
                } else {
                    resolve_package_entry(&nm)?
                }
            } else {
                // Try exports field first
                let export_key = format!("./{subpath}");
                if let Some(resolved) = resolve_package_exports(&nm, &export_key, esm) {
                    resolved
                } else {
                    let sub = nm.join(subpath);
                    resolve_as_file_or_directory(&sub).ok_or_else(|| {
                        format!("Cannot find '{specifier}' in {}", nm.display())
                    })?
                }
            };
            return Ok(target);
        }
        dir = d.parent();
    }

    Err(format!(
        "Cannot find module '{specifier}' from '{}'",
        from_dir.display()
    ))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Normalize a path by resolving `.` and `..` components without touching the filesystem.
/// Falls back to canonicalize for symlinks, or returns the logical normalization.
fn normalize_path(path: &Path) -> Option<PathBuf> {
    // Try fs::canonicalize first (resolves symlinks + normalizes)
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Some(canonical);
    }
    // Fallback: manual normalization
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => { components.pop(); }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    let result: PathBuf = components.iter().collect();
    if result.as_os_str().is_empty() { None } else { Some(result) }
}

/// Parse a specifier into (package_name, subpath).
/// E.g. "koa/lib" -> ("koa", "lib"), "@koa/router/lib" -> ("@koa/router", "lib")
fn parse_package_name(specifier: &str) -> (&str, &str) {
    if specifier.starts_with('@') {
        // Scoped: @scope/name[/subpath]
        if let Some(first_slash) = specifier[1..].find('/') {
            let after_scope = 1 + first_slash + 1;
            if after_scope < specifier.len() {
                if let Some(second_slash) = specifier[after_scope..].find('/') {
                    let split = after_scope + second_slash;
                    return (&specifier[..split], &specifier[split + 1..]);
                }
            }
        }
        (specifier, "")
    } else {
        match specifier.find('/') {
            Some(pos) => (&specifier[..pos], &specifier[pos + 1..]),
            None => (specifier, ""),
        }
    }
}

/// Resolve a subpath via the package.json "exports" field.
/// Returns None if exports field is absent or doesn't match.
fn resolve_package_exports(pkg_dir: &Path, export_key: &str, esm: bool) -> Option<PathBuf> {
    let pkg_json = pkg_dir.join("package.json");
    let content = std::fs::read_to_string(&pkg_json).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let exports = json.get("exports")?;

    // exports can be a string (shorthand for ".") or an object
    if let Some(s) = exports.as_str() {
        if export_key == "." {
            let resolved = pkg_dir.join(s);
            return resolve_as_file_or_directory(&resolved);
        }
        return None;
    }

    let exports_obj = exports.as_object()?;

    // Try exact match first
    if let Some(target) = resolve_export_target(pkg_dir, exports_obj.get(export_key), esm) {
        return Some(target);
    }

    // Try wildcard patterns (e.g., "./*")
    for (pattern, value) in exports_obj {
        if let Some(prefix) = pattern.strip_suffix('*') {
            if export_key.starts_with(prefix) {
                let rest = &export_key[prefix.len()..];
                if let Some(target_pattern) = extract_export_string(value) {
                    if let Some(resolved_str) = target_pattern.strip_suffix('*') {
                        let resolved_path = format!("{resolved_str}{rest}");
                        let full = pkg_dir.join(&resolved_path);
                        if let Some(r) = resolve_as_file_or_directory(&full) {
                            return Some(r);
                        }
                    } else if target_pattern.contains('*') {
                        let resolved_path = target_pattern.replace('*', rest);
                        let full = pkg_dir.join(&resolved_path);
                        if let Some(r) = resolve_as_file_or_directory(&full) {
                            return Some(r);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Given an exports target value, resolve it to a file path.
/// Handles: string, or conditional object with "require"/"default"/"node" keys.
fn resolve_export_target(
    pkg_dir: &Path,
    value: Option<&serde_json::Value>,
    esm: bool,
) -> Option<PathBuf> {
    let value = value?;
    if let Some(s) = value.as_str() {
        return resolve_as_file_or_directory(&pkg_dir.join(s));
    }
    if let Some(obj) = value.as_object() {
        // ESM imports prefer "import"; CJS requires prefer "require". Both fall
        // back to "node" then "default".
        let keys: &[&str] = if esm {
            &["import", "node", "default"]
        } else {
            &["require", "node", "default"]
        };
        for key in keys {
            if let Some(v) = obj.get(*key) {
                // Recurse into nested condition objects
                if let Some(r) = resolve_export_target(pkg_dir, Some(v), esm) {
                    return Some(r);
                }
            }
        }
    }
    None
}

/// Extract a string from an exports value, handling conditional objects recursively.
fn extract_export_string(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(obj) = value.as_object() {
        for key in &["require", "node", "default"] {
            if let Some(v) = obj.get(*key) {
                if let Some(s) = extract_export_string(v) {
                    return Some(s);
                }
            }
        }
    }
    None
}

/// Resolve the main entry point of a package directory.
fn resolve_package_entry(pkg_dir: &Path) -> Result<PathBuf, String> {
    let pkg_json = pkg_dir.join("package.json");
    if pkg_json.is_file() {
        if let Ok(content) = std::fs::read_to_string(&pkg_json) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
            {
                if let Some(main) = json.get("main").and_then(|v| v.as_str()) {
                    let main_path = pkg_dir.join(main);
                    if let Some(resolved) = resolve_as_file_or_directory(&main_path)
                    {
                        return Ok(resolved);
                    }
                }
            }
        }
    }

    // Fallback: index files
    for name in &["index.js", "index.ts", "index.json"] {
        let candidate = pkg_dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "Cannot find entry point for package at {}",
        pkg_dir.display()
    ))
}

/// Append an extension to a path (unlike `with_extension`, this doesn't replace).
fn with_added_extension(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

/// Try to resolve a path as a file, then as a directory.
/// Tries: path, path.js, path.ts, path.json, path.cjs, path/index.js, etc.
fn resolve_as_file_or_directory(path: &Path) -> Option<PathBuf> {
    // Try as file directly
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    // Try with extensions appended
    for ext in &["js", "ts", "json", "cjs", "jsx", "tsx"] {
        let with_ext = with_added_extension(path, ext);
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }

    // Try as directory
    if path.is_dir() {
        // Check package.json "main" field first
        let pkg_json = path.join("package.json");
        if pkg_json.is_file() {
            if let Ok(content) = std::fs::read_to_string(&pkg_json) {
                if let Ok(json) =
                    serde_json::from_str::<serde_json::Value>(&content)
                {
                    if let Some(main) = json.get("main").and_then(|v| v.as_str())
                    {
                        let main_path = path.join(main);
                        if let Some(resolved) =
                            resolve_as_file_or_directory(&main_path)
                        {
                            return Some(resolved);
                        }
                    }
                }
            }
        }

        // Try index files
        for name in &["index.js", "index.ts", "index.json"] {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn builtin_detection() {
        assert!(is_node_builtin("fs"));
        assert!(is_node_builtin("node:path"));
        assert!(is_node_builtin("fs/promises"));
        assert!(!is_node_builtin("koa"));
        assert!(!is_node_builtin("express"));
    }

    #[test]
    fn resolve_relative_file() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib.js");
        fs::write(&lib, "module.exports = 42;").unwrap();

        let path = dir.path().join("lib");
        let resolved = resolve_as_file_or_directory(&path).unwrap();
        assert_eq!(resolved, lib);
    }

    #[test]
    fn detect_cjs_extension() {
        assert!(is_cjs(Path::new("/foo/bar.cjs")));
        assert!(!is_cjs(Path::new("/foo/bar.mjs")));
    }

    #[test]
    fn detect_cjs_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("package.json");
        let file = dir.path().join("index.js");

        // No type field -> CJS
        fs::write(&pkg, r#"{"name": "test"}"#).unwrap();
        fs::write(&file, "").unwrap();
        assert!(is_cjs(&file));

        // type: module -> not CJS
        fs::write(&pkg, r#"{"type": "module"}"#).unwrap();
        assert!(!is_cjs(&file));
    }

    #[test]
    fn parse_package_names() {
        assert_eq!(parse_package_name("koa"), ("koa", ""));
        assert_eq!(parse_package_name("koa/lib/app"), ("koa", "lib/app"));
        assert_eq!(parse_package_name("@koa/router"), ("@koa/router", ""));
        assert_eq!(
            parse_package_name("@koa/router/lib"),
            ("@koa/router", "lib")
        );
    }

    #[test]
    fn resolve_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/testpkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.js"), "module.exports = {};").unwrap();

        let result =
            resolve_from_node_modules_dir("testpkg", dir.path(), false).unwrap();
        assert_eq!(result, nm.join("index.js"));
    }

    #[test]
    fn resolve_node_modules_main_field() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/mypkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(
            nm.join("package.json"),
            r#"{"name": "mypkg", "main": "./lib/entry.js"}"#,
        )
        .unwrap();
        let lib = nm.join("lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("entry.js"), "module.exports = {};").unwrap();

        let result =
            resolve_from_node_modules_dir("mypkg", dir.path(), false).unwrap();
        assert_eq!(result, lib.join("entry.js"));
    }
}
