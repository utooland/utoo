// CJS Loader - provides globalThis.require for CommonJS modules.
// Loaded as a non-ESM bootstrap script so it runs before any modules.
const __cjsOps = Deno.core.ops;

const __cjs_cache = new Map();
const __cjs_builtins = new Map();
globalThis.__cjs_cache = __cjs_cache;
globalThis.__cjs_builtins = __cjs_builtins;
globalThis.__cjs_current_file = "";

function __cjs_require(specifier) {
  // 1. Resolve
  const resolved = __cjsOps.op_cjs_resolve(
    specifier,
    globalThis.__cjs_current_file,
  );

  // 2. Built-in check ("node:fs" -> look up __cjs_builtins)
  if (resolved.startsWith("node:")) {
    const name = resolved.slice(5);
    if (__cjs_builtins.has(resolved)) return __cjs_builtins.get(resolved);
    if (__cjs_builtins.has(name)) return __cjs_builtins.get(name);
    throw new Error(`Cannot find built-in module '${resolved}'`);
  }

  // 3. Cache hit (also handles circular deps - returns partial exports)
  if (__cjs_cache.has(resolved)) return __cjs_cache.get(resolved).exports;

  // 4. JSON shortcut
  if (resolved.endsWith(".json")) {
    const obj = JSON.parse(__cjsOps.op_fs_read_text_file_sync(resolved));
    __cjs_cache.set(resolved, { exports: obj });
    return obj;
  }

  // 5. Read source + optional transpile
  let source = __cjsOps.op_fs_read_text_file_sync(resolved);
  if (/\.(ts|tsx)$/.test(resolved)) {
    source = __cjsOps.op_cjs_transpile(source, resolved);
  }

  // 6. Pre-cache (circular dep safety), wrap, eval
  const mod = {
    exports: {},
    id: resolved,
    filename: resolved,
    loaded: false,
  };
  __cjs_cache.set(resolved, mod);

  const dirname = resolved.substring(0, resolved.lastIndexOf("/"));

  // Create a per-module require that captures this module's path
  function moduleRequire(spec) {
    const prev = globalThis.__cjs_current_file;
    globalThis.__cjs_current_file = resolved;
    try {
      return __cjs_require(spec);
    } finally {
      globalThis.__cjs_current_file = prev;
    }
  }
  moduleRequire.resolve = function (spec) {
    return __cjsOps.op_cjs_resolve(spec, resolved);
  };
  moduleRequire.cache = __cjs_cache;
  moduleRequire.main = undefined;
  moduleRequire.extensions = {
    ".js": function(mod, filename) {},
    ".json": function(mod, filename) {},
    ".node": function(mod, filename) {},
    ".ts": function(mod, filename) {},
    ".cjs": function(mod, filename) {},
  };

  const wrapped =
    "(function(require,module,exports,__filename,__dirname){\n" +
    source +
    "\n})";
  let fn_;
  try {
    fn_ = (0, eval)(wrapped);
  } catch (e) {
    Deno.core.ops.op_console_error("[cjs_loader] parse error in " + resolved + ": " + e.message);
    throw e;
  }

  const prev = globalThis.__cjs_current_file;
  globalThis.__cjs_current_file = resolved;
  try {
    fn_(moduleRequire, mod, mod.exports, resolved, dirname);
  } catch (e) {
    Deno.core.ops.op_console_error("[cjs_loader] runtime error in " + resolved + ": " + e.message);
    throw e;
  }
  globalThis.__cjs_current_file = prev;
  mod.loaded = true;

  return mod.exports;
}

__cjs_require.resolve = function (spec) {
  return __cjsOps.op_cjs_resolve(spec, globalThis.__cjs_current_file);
};
__cjs_require.cache = __cjs_cache;

globalThis.require = __cjs_require;
