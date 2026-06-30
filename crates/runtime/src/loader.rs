use std::path::Path;

use deno_core::{
    ModuleLoadResponse, ModuleSourceCode, ModuleType, ResolutionKind,
    error::ModuleLoaderError,
    resolve_import,
};
use deno_error::JsErrorBox;

use crate::ops::cjs;
use crate::transpile::transpile_to_js;

pub struct UtooModuleLoader;

/// Maps `node:*` and bare built-in specifiers to internal `ext:` URLs.
fn resolve_builtin(specifier: &str) -> Option<String> {
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    match name {
        "fs" | "fs/promises" | "path" | "os" | "url" | "buffer" | "events" | "util" | "assert"
        | "querystring" | "string_decoder" | "stream" | "net" | "http" | "https" | "http2"
        | "async_hooks" | "crypto" | "zlib" | "v8" | "cluster" | "child_process" | "tty"
        | "dns" | "dgram" | "tls" | "worker_threads" | "perf_hooks" | "module"
        | "readline" | "diagnostics_channel" | "console" | "timers"
        | "timers/promises" | "constants" | "domain"
        | "util/types" | "stream/promises" | "stream/web" | "stream/consumers"
        | "inspector" | "dns/promises" | "path/posix" | "vm" | "repl" => {
            let normalized = name.replace('/', "_");
            Some(format!("ext:utoo_rt_ext/node/{normalized}"))
        }
        _ => None,
    }
}

/// Returns true if the specifier is a bare specifier (not relative, absolute, or URL).
fn is_bare_specifier(specifier: &str) -> bool {
    !specifier.starts_with('.')
        && !specifier.starts_with('/')
        && !specifier.contains("://")
}

/// Resolve a bare specifier by walking up node_modules from the referrer's directory.
fn resolve_from_node_modules(
    specifier: &str,
    referrer: &str,
) -> Result<deno_core::ModuleSpecifier, ModuleLoaderError> {
    let referrer_url = deno_core::ModuleSpecifier::parse(referrer)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let referrer_path = referrer_url
        .to_file_path()
        .map_err(|_| JsErrorBox::generic(format!("Not a file URL: {referrer}")))?;
    let referrer_dir = referrer_path
        .parent()
        .unwrap_or(Path::new("."));

    let resolved = cjs::resolve_from_node_modules_dir(specifier, referrer_dir, true)
        .map_err(|e| JsErrorBox::generic(e))?;

    deno_core::ModuleSpecifier::from_file_path(&resolved).map_err(|_| {
        JsErrorBox::generic(format!(
            "Cannot convert to URL: {}",
            resolved.display()
        ))
    })
}

/// Quick check if source contains ESM syntax (import/export declarations).
/// If true, the file should not be wrapped in a function scope.
fn has_esm_syntax(source: &str) -> bool {
    // Check for import/export at start of line (common ESM patterns)
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") || trimmed.starts_with("import{") {
            return true;
        }
        if trimmed.starts_with("export ") || trimmed.starts_with("export{") {
            return true;
        }
    }
    false
}

/// Wrap CJS source code as an ESM shim so deno_core can evaluate it.
fn wrap_cjs_as_esm(source: &str, path: &Path) -> String {
    let abs_path = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let abs_dir = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    // If source has ESM syntax (import/export), it was misdetected as CJS.
    // Keep it at module level so import/export work (they're hoisted by V8).
    if has_esm_syntax(source) {
        return format!(
            r#"const __filename = "{abs_path}";
const __dirname = "{abs_dir}";
const module = {{ exports: {{}} }};
const exports = module.exports;
const require = globalThis.require;
const __prev = globalThis.__cjs_current_file || "";
globalThis.__cjs_current_file = __filename;
if (globalThis.__cjs_cache) globalThis.__cjs_cache.set(__filename, module);
{source}
globalThis.__cjs_current_file = __prev;
export default module.exports;
"#
        );
    }

    // True CJS: wrap in function to isolate variables, extract named exports
    let export_names = extract_cjs_export_names(source);
    let named_exports = if export_names.is_empty() {
        String::new()
    } else {
        let names = export_names.join(", ");
        format!(
            "const {{ {names} }} = __mod.exports;\nexport {{ {names} }};\n"
        )
    };

    format!(
        r#"const __filename = "{abs_path}";
const __dirname = "{abs_dir}";
const __mod = {{ exports: {{}} }};
const __prev = globalThis.__cjs_current_file || "";
globalThis.__cjs_current_file = __filename;
if (globalThis.__cjs_cache) globalThis.__cjs_cache.set(__filename, __mod);
(function(module, exports, require, __filename, __dirname) {{
{source}
}}).call(__mod.exports, __mod, __mod.exports, globalThis.require, __filename, __dirname);
globalThis.__cjs_current_file = __prev;
export default __mod.exports;
{named_exports}"#
    )
}

/// Extract export names from CJS source code for ESM re-export generation.
/// Detects common patterns used by bundlers like SWC and webpack.
fn extract_cjs_export_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();

    // Pattern 1: 0 && (module.exports = { key: ..., ... })
    // SWC static analysis hint (dead code that lists all exports)
    for marker in &[
        "0 && (module.exports = {",
        "0&&(module.exports={",
    ] {
        if let Some(pos) = source.find(marker) {
            let brace_start = pos + marker.len() - 1;
            collect_object_keys(source, brace_start, &mut names);
            if !names.is_empty() {
                break;
            }
        }
    }

    // Pattern 2: _export(exports, { ... }) or __export(exports, { ... })
    if names.is_empty() {
        for marker in &[
            "_export(exports, {",
            "__export(exports, {",
            "_export(exports,{",
            "__export(exports,{",
        ] {
            if let Some(pos) = source.find(marker) {
                let brace_start = pos + marker.len() - 1;
                collect_object_keys(source, brace_start, &mut names);
                if !names.is_empty() {
                    break;
                }
            }
        }
    }

    // Pattern 3: Object.defineProperty(exports, "name", ...)
    if names.is_empty() {
        let mut search_from = 0;
        while search_from < source.len() {
            let haystack = &source[search_from..];
            let found = haystack
                .find("Object.defineProperty(exports,")
                .or_else(|| haystack.find("Object.defineProperty(exports ,"));
            match found {
                Some(pos) => {
                    let abs = search_from + pos;
                    // Find the opening paren's second arg (the property name)
                    if let Some(comma) = source[abs..].find(',') {
                        let after_comma = abs + comma + 1;
                        let rest = source[after_comma..].trim_start();
                        if let Some(name) = extract_quoted_string(rest) {
                            if !name.is_empty() {
                                names.push(name);
                            }
                        }
                    }
                    search_from = abs + 30; // skip past this match
                }
                None => break,
            }
        }
    }

    // Filter out internal/problematic names
    names.retain(|n| {
        !n.is_empty()
            && n != "__esModule"
            && n != "default"
            && is_valid_js_identifier(n)
    });
    names.sort();
    names.dedup();
    names
}

/// Collect top-level keys from an object literal starting at the `{`.
fn collect_object_keys(source: &str, brace_pos: usize, names: &mut Vec<String>) {
    let bytes = source.as_bytes();
    let len = bytes.len();
    if brace_pos >= len || bytes[brace_pos] != b'{' {
        return;
    }

    let mut i = brace_pos + 1;
    let mut depth: i32 = 1;

    while i < len && depth > 0 {
        let b = bytes[i];
        match b {
            b'{' | b'(' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b')' | b']' => {
                depth -= 1;
                i += 1;
            }
            b'"' | b'\'' | b'`' => {
                // Skip string/template literal
                let quote = b;
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'/' if i + 1 < len => {
                if bytes[i + 1] == b'/' {
                    // Line comment
                    i += 2;
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else if bytes[i + 1] == b'*' {
                    // Block comment
                    i += 2;
                    while i + 1 < len
                        && !(bytes[i] == b'*' && bytes[i + 1] == b'/')
                    {
                        i += 1;
                    }
                    if i + 1 < len {
                        i += 2;
                    }
                } else {
                    i += 1;
                }
            }
            _ if depth == 1 => {
                // Top level of object -- look for identifier keys
                if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
                    let key_start = i;
                    while i < len
                        && (bytes[i].is_ascii_alphanumeric()
                            || bytes[i] == b'_'
                            || bytes[i] == b'$')
                    {
                        i += 1;
                    }
                    let key = &source[key_start..i];
                    // Skip whitespace
                    while i < len && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    // If followed by `:`, it's an object key
                    if i < len && bytes[i] == b':' {
                        names.push(key.to_string());
                    }
                    // continue without incrementing (already advanced)
                } else if b == b'"' || b == b'\'' {
                    // Quoted key at top level
                    let quote = b;
                    i += 1;
                    let key_start = i;
                    while i < len && bytes[i] != quote {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    let key = source[key_start..i].to_string();
                    if i < len {
                        i += 1; // skip closing quote
                    }
                    while i < len && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < len && bytes[i] == b':' {
                        names.push(key);
                    }
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Extract a quoted string value (first char must be `"` or `'`).
fn extract_quoted_string(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let quote = bytes[0];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return Some(s[1..i].to_string());
        }
        i += 1;
    }
    None
}

/// Check if a string is a valid JS identifier (ASCII subset).
fn is_valid_js_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

impl deno_core::ModuleLoader for UtooModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<deno_core::ModuleSpecifier, ModuleLoaderError> {
        // 1. Built-in (existing)
        if let Some(mapped) = resolve_builtin(specifier) {
            return deno_core::ModuleSpecifier::parse(&mapped)
                .map_err(|e| JsErrorBox::generic(e.to_string()));
        }

        // 2. Handle relative imports from eval'd CJS code.
        //    When import() is called inside eval'd CJS, the referrer is the ext:
        //    URL of cjs_loader.js which can't resolve relative paths. Use the
        //    tracked CJS file path as the base instead.
        if specifier.starts_with('.')
            && (referrer.starts_with("ext:") || referrer.is_empty())
        {
            let cjs_file = cjs::CURRENT_CJS_FILE.with(|f| f.borrow().clone());
            if !cjs_file.is_empty() {
                if let Ok(base_url) =
                    deno_core::ModuleSpecifier::from_file_path(&cjs_file)
                {
                    return resolve_import(specifier, base_url.as_str())
                        .map_err(|e| JsErrorBox::generic(e.to_string()));
                }
            }
        }

        // 3. Standard ESM resolution
        match resolve_import(specifier, referrer) {
            Ok(r) => Ok(r),
            // 4. Fallback: node_modules walk for bare specifiers
            Err(_) if is_bare_specifier(specifier) => {
                resolve_from_node_modules(specifier, referrer)
            }
            Err(e) => Err(JsErrorBox::generic(e.to_string())),
        }
    }

    fn load(
        &self,
        module_specifier: &deno_core::ModuleSpecifier,
        _maybe_referrer: Option<&deno_core::ModuleLoadReferrer>,
        _options: deno_core::ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let specifier = module_specifier.clone();
        if specifier.scheme() == "ext" {
            return ModuleLoadResponse::Sync(Err(JsErrorBox::generic(format!(
                "Built-in module not found: {specifier}"
            ))));
        }
        ModuleLoadResponse::Sync(load_from_disk(&specifier))
    }
}

fn load_from_disk(
    specifier: &deno_core::ModuleSpecifier,
) -> Result<deno_core::ModuleSource, ModuleLoaderError> {
    let path = specifier
        .to_file_path()
        .map_err(|_| JsErrorBox::generic(format!("Not a file URL: {specifier}")))?;

    // Native addon files (.node) -- load via NAPI op
    if path.extension().and_then(|e| e.to_str()) == Some("node") {
        let abs_path = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let shim = format!(
            r#"const exports = Deno.core.ops.op_napi_open(
  "{abs_path}",
  globalThis,
  function createBuffer(ab) {{ return new Uint8Array(ab); }},
  function reportError(err) {{ throw err; }},
);
export default exports;
"#
        );
        return Ok(deno_core::ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::String(shim.into()),
            specifier,
            None,
        ));
    }

    let source = std::fs::read_to_string(&path)
        .map_err(|e| JsErrorBox::from_err(e))?;

    // Strip shebang line (e.g. #!/usr/bin/env node)
    let source = if source.starts_with("#!") {
        match source.find('\n') {
            Some(pos) => source[pos + 1..].to_string(),
            None => String::new(),
        }
    } else {
        source
    };

    let code = if needs_transpile(&path) {
        transpile_to_js(&source, &path)
            .map_err(|e| JsErrorBox::generic(e.to_string()))?
    } else {
        source
    };

    // Detect CJS and wrap as ESM shim
    if cjs::is_cjs(&path) {
        let wrapped = wrap_cjs_as_esm(&code, &path);
        return Ok(deno_core::ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::String(wrapped.into()),
            specifier,
            None,
        ));
    }

    Ok(deno_core::ModuleSource::new(
        ModuleType::JavaScript,
        ModuleSourceCode::String(code.into()),
        specifier,
        None,
    ))
}

fn needs_transpile(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "jsx")
    )
}
