// CJS Loader - provides globalThis.require for CommonJS modules.
// Loaded as a non-ESM bootstrap script so it runs before any modules.
const __cjsOps = Deno.core.ops;

const __cjs_cache_map = new Map();
// Wrap the Map in a Proxy so that bracket access (require.cache[key])
// works like Node.js (which uses a plain object for require.cache).
// Next.js and other frameworks use require.cache[key] and
// delete require.cache[key] to manage the module cache.
const __cjs_cache = new Proxy(__cjs_cache_map, {
  get(map, key) {
    if (key === Symbol.iterator) return map[Symbol.iterator].bind(map);
    if (typeof key === 'symbol') return map[key];
    // Proxy Map methods
    if (key === 'has') return map.has.bind(map);
    if (key === 'get') return map.get.bind(map);
    if (key === 'set') return map.set.bind(map);
    if (key === 'delete') return map.delete.bind(map);
    if (key === 'clear') return map.clear.bind(map);
    if (key === 'size') return map.size;
    if (key === 'keys') return map.keys.bind(map);
    if (key === 'values') return map.values.bind(map);
    if (key === 'entries') return map.entries.bind(map);
    if (key === 'forEach') return map.forEach.bind(map);
    // Bracket access: require.cache[filepath] -> map.get(filepath)
    return map.get(key);
  },
  set(map, key, value) {
    if (typeof key === 'symbol') { map[key] = value; return true; }
    map.set(key, value);
    return true;
  },
  has(map, key) {
    if (typeof key === 'symbol') return key in map;
    return map.has(key);
  },
  deleteProperty(map, key) {
    return map.delete(key);
  },
  ownKeys(map) {
    return [...map.keys()];
  },
  getOwnPropertyDescriptor(map, key) {
    if (map.has(key)) {
      return { configurable: true, enumerable: true, value: map.get(key), writable: true };
    }
    return undefined;
  },
});
const __cjs_builtins = new Map();
globalThis.__cjs_cache = __cjs_cache;
globalThis.__cjs_builtins = __cjs_builtins;
globalThis.__cjs_current_file = "";

function __cjs_require(specifier) {
  // 1. Resolve
  let resolved;
  try {
    resolved = __cjsOps.op_cjs_resolve(
      specifier,
      globalThis.__cjs_current_file,
    );
  } catch (e) {
    if (e && !e.code && e.message && e.message.includes("Cannot find module")) {
      e.code = "MODULE_NOT_FOUND";
    }
    throw e;
  }

  // 2. Built-in check ("node:fs" -> look up __cjs_builtins)
  if (resolved.startsWith("node:")) {
    const name = resolved.slice(5);
    if (__cjs_builtins.has(resolved)) return __cjs_builtins.get(resolved);
    if (__cjs_builtins.has(name)) return __cjs_builtins.get(name);
    throw new Error(`Cannot find built-in module '${resolved}'`);
  }

  // 3. Cache hit (also handles circular deps - returns partial exports)
  if (__cjs_cache.has(resolved)) return __cjs_cache.get(resolved).exports;

  // 4. Native addon (.node) - load via NAPI
  if (resolved.endsWith(".node")) {
    const exports = __cjsOps.op_napi_open(
      resolved,
      globalThis,
      function createBuffer(ab) { return new Uint8Array(ab); },
      function reportError(err) { throw err; },
    );
    __cjs_cache.set(resolved, { id: resolved, filename: resolved, exports, loaded: true, children: [], parent: null });
    return exports;
  }

  // 5. JSON shortcut
  if (resolved.endsWith(".json")) {
    const obj = JSON.parse(__cjsOps.op_fs_read_text_file_sync(resolved));
    __cjs_cache.set(resolved, { id: resolved, filename: resolved, exports: obj, loaded: true, children: [], parent: null });
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
    children: [],
    parent: null,
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
