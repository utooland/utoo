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
        | "querystring" | "string_decoder" | "stream" | "net" | "http" | "https"
        | "async_hooks" | "crypto" | "zlib" | "v8" | "cluster" | "child_process" | "tty"
        | "dns" | "dgram" | "tls" | "worker_threads" | "perf_hooks" | "module"
        | "readline" | "diagnostics_channel" | "console" | "timers"
        | "timers/promises" | "constants" | "domain"
        | "util/types" | "stream/promises" | "stream/web" | "stream/consumers"
        | "inspector" => {
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

    let resolved = cjs::resolve_from_node_modules_dir(specifier, referrer_dir)
        .map_err(|e| JsErrorBox::generic(e))?;

    deno_core::ModuleSpecifier::from_file_path(&resolved).map_err(|_| {
        JsErrorBox::generic(format!(
            "Cannot convert to URL: {}",
            resolved.display()
        ))
    })
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

    format!(
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
    )
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

        // 2. Standard ESM resolution
        match resolve_import(specifier, referrer) {
            Ok(r) => Ok(r),
            // 3. Fallback: node_modules walk for bare specifiers
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

    let source = std::fs::read_to_string(&path)
        .map_err(|e| JsErrorBox::from_err(e))?;

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
