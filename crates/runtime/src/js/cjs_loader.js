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

// SWC helpers: global functions used by transpiled TypeScript/ESM->CJS code.
// SWC's common_js transform emits bare calls like _interop_require_default(require("x"))
// and the decorator transform emits _ts_decorate([...], proto, key, desc).
// These must be available as globals since inject_helpers is skipped (it produces ESM imports).
globalThis._interop_require_default = function(obj) {
  return obj && obj.__esModule ? obj : { default: obj };
};
globalThis._interop_require_wildcard = function(obj, nodeInterop) {
  if (!nodeInterop && obj && obj.__esModule) return obj;
  if (obj === null || (typeof obj !== "object" && typeof obj !== "function")) return { default: obj };
  var newObj = {};
  var hasPropertyDescriptor = Object.defineProperty && Object.getOwnPropertyDescriptor;
  for (var key in obj) {
    if (key !== "default" && Object.prototype.hasOwnProperty.call(obj, key)) {
      var desc = hasPropertyDescriptor ? Object.getOwnPropertyDescriptor(obj, key) : null;
      if (desc && (desc.get || desc.set)) Object.defineProperty(newObj, key, desc);
      else newObj[key] = obj[key];
    }
  }
  newObj.default = obj;
  return newObj;
};
globalThis._ts_decorate = function(decorators, target, key, desc) {
  var c = arguments.length,
    r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
  // SWC uses 0 as placeholder in decorator arrays (e.g. [0, Inject()]).
  // Reflect.decorate doesn't filter falsy entries, so always use the manual loop
  // which skips them via `if (d = decorators[i])`.
  for (var i = decorators.length - 1; i >= 0; i--)
    if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
  return c > 3 && r && Object.defineProperty(target, key, r), r;
};
globalThis._export_star = function(from, to) {
  Object.keys(from).forEach(function(k) {
    if (k !== "default" && !Object.prototype.hasOwnProperty.call(to, k)) {
      Object.defineProperty(to, k, { enumerable: true, get: function() { return from[k]; } });
    }
  });
  return from;
};
globalThis._ts_metadata = function(k, v) {
  if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(k, v);
};

// Also provide via @swc/helpers/_/<name> require pattern (for code that uses inject_helpers)
const __swc_helpers = {
  _ts_decorate: function(decorators, target, key, desc) {
    var c = arguments.length,
      r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    for (var i = decorators.length - 1; i >= 0; i--)
      if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
  },
  _interop_require_default: function(obj) {
    return obj && obj.__esModule ? obj : { default: obj };
  },
  _interop_require_wildcard: function(obj, nodeInterop) {
    if (!nodeInterop && obj && obj.__esModule) return obj;
    if (obj === null || (typeof obj !== "object" && typeof obj !== "function")) return { default: obj };
    var cache = typeof WeakMap === "function" ? new WeakMap() : undefined;
    var newObj = {};
    var hasPropertyDescriptor = Object.defineProperty && Object.getOwnPropertyDescriptor;
    for (var key in obj) {
      if (key !== "default" && Object.prototype.hasOwnProperty.call(obj, key)) {
        var desc = hasPropertyDescriptor ? Object.getOwnPropertyDescriptor(obj, key) : null;
        if (desc && (desc.get || desc.set)) Object.defineProperty(newObj, key, desc);
        else newObj[key] = obj[key];
      }
    }
    newObj.default = obj;
    return newObj;
  },
  _class_call_check: function(instance, Constructor) {
    if (!(instance instanceof Constructor)) throw new TypeError("Cannot call a class as a function");
  },
  _define_property: function(obj, key, value) {
    if (key in obj) Object.defineProperty(obj, key, { value: value, enumerable: true, configurable: true, writable: true });
    else obj[key] = value;
    return obj;
  },
  _object_spread: function(target) {
    for (var i = 1; i < arguments.length; i++) {
      var source = arguments[i] != null ? arguments[i] : {};
      var keys = Object.keys(source);
      if (typeof Object.getOwnPropertySymbols === "function")
        keys.push.apply(keys, Object.getOwnPropertySymbols(source).filter(function(sym) { return Object.getOwnPropertyDescriptor(source, sym).enumerable; }));
      keys.forEach(function(key) { __swc_helpers._define_property(target, key, source[key]); });
    }
    return target;
  },
  _object_spread_props: function(target, source) {
    source = source != null ? source : {};
    if (Object.getOwnPropertyDescriptors) Object.defineProperties(target, Object.getOwnPropertyDescriptors(source));
    else {
      var keys = Object.keys(source);
      for (var i = 0; i < keys.length; i++) {
        var key = keys[i];
        Object.defineProperty(target, key, Object.getOwnPropertyDescriptor(source, key));
      }
    }
    return target;
  },
  _async_to_generator: function(fn) {
    return function() {
      var self = this, args = arguments;
      return new Promise(function(resolve, reject) {
        var gen = fn.apply(self, args);
        function _next(value) { var info; try { info = gen.next(value); } catch (e) { reject(e); return; } if (info.done) resolve(info.value); else Promise.resolve(info.value).then(_next, _throw); }
        function _throw(value) { var info; try { info = gen.throw(value); } catch (e) { reject(e); return; } if (info.done) resolve(info.value); else Promise.resolve(info.value).then(_next, _throw); }
        _next(undefined);
      });
    };
  },
  _ts_metadata: function(k, v) {
    if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(k, v);
  },
};

function __cjs_require(specifier) {
  // 0a. Fast path for @swc/helpers
  if (specifier.startsWith("@swc/helpers/_/")) {
    var helperName = specifier.substring(15); // after "@swc/helpers/_/"
    var fn = __swc_helpers[helperName];
    if (fn) return { _: fn };
    // Fallback: return a no-op helper
    return { _: function() {} };
  }

  // 0. Fast path for "node:" prefixed builtins (before Rust resolve, which may throw)
  if (specifier.startsWith("node:") || __cjs_builtins.has(specifier)) {
    var __bn = specifier.startsWith("node:") ? specifier.slice(5) : specifier;
    if (__cjs_builtins.has(specifier)) return __cjs_builtins.get(specifier);
    if (__cjs_builtins.has(__bn)) return __cjs_builtins.get(__bn);
    // Handle sub-module paths: "node:assert/strict" -> assert.strict,
    // "node:fs/promises" -> fs.promises, "node:stream/promises" -> stream.promises, etc.
    var __si = __bn.indexOf("/");
    if (__si !== -1) {
      var __base = __bn.substring(0, __si);
      var __sub = __bn.substring(__si + 1);
      var __bmod = __cjs_builtins.get(__base) || __cjs_builtins.get("node:" + __base);
      if (__bmod && __bmod[__sub] !== undefined) return __bmod[__sub];
      if (__bmod) return __bmod;
    }
  }

  // 1. Resolve (use Module._resolveFilename if available, so hooks like tsconfig-paths work)
  let resolved;
  try {
    var __MC = globalThis.__cjs_builtins && globalThis.__cjs_builtins.get("module");
    if (__MC && typeof __MC._resolveFilename === "function") {
      resolved = __MC._resolveFilename(specifier, {
        filename: globalThis.__cjs_current_file,
        id: globalThis.__cjs_current_file,
      });
    } else {
      resolved = __cjsOps.op_cjs_resolve(
        specifier,
        globalThis.__cjs_current_file,
      );
    }
  } catch (e) {
    if (e && !e.code && e.message && e.message.includes("Cannot find module")) {
      e.code = "MODULE_NOT_FOUND";
    }
    throw e;
  }

  // 2. Built-in check (for cases where Rust resolver returns "node:xxx")
  if (resolved.startsWith("node:")) {
    var name = resolved.slice(5);
    if (__cjs_builtins.has(resolved)) return __cjs_builtins.get(resolved);
    if (__cjs_builtins.has(name)) return __cjs_builtins.get(name);
    throw new Error(`Cannot find built-in module '${resolved}'`);
  }

  // 3. Cache hit (also handles circular deps - returns partial exports)
  if (__cjs_cache.has(resolved)) return __cjs_cache.get(resolved).exports;

  // 4. Native addon (.node) - load via NAPI
  if (resolved.endsWith(".node")) {
    try {
      const exports = __cjsOps.op_napi_open(
        resolved,
        globalThis,
        function createBuffer(ab) { return new Uint8Array(ab); },
        function reportError(err) { throw err; },
      );
      __cjs_cache.set(resolved, { id: resolved, filename: resolved, exports, loaded: true, children: [], parent: null });
      return exports;
    } catch (e) {
      Deno.core.ops.op_console_error("[cjs_loader] native addon skipped: " + resolved + " (" + e.message + ")");
      // Return a Proxy that acts as a no-op for any property access / function call
      const handler = {
        get(target, prop) {
          if (prop === Symbol.toPrimitive) return () => 0;
          if (prop === "toString") return () => "[native addon stub]";
          if (prop === Symbol.toStringTag) return "NativeAddonStub";
          return function() {};
        }
      };
      const stubExports = new Proxy({}, handler);
      __cjs_cache.set(resolved, { id: resolved, filename: resolved, exports: stubExports, loaded: true, children: [], parent: null });
      return stubExports;
    }
  }

  // 5. JSON shortcut
  if (resolved.endsWith(".json")) {
    const obj = JSON.parse(__cjsOps.op_fs_read_text_file_sync(resolved));
    __cjs_cache.set(resolved, { id: resolved, filename: resolved, exports: obj, loaded: true, children: [], parent: null });
    return obj;
  }

  // 5. Read source + optional transpile
  let source = __cjsOps.op_fs_read_text_file_sync(resolved);
  // Strip shebang line (e.g. #!/usr/bin/env node)
  if (source.charCodeAt(0) === 0x23 && source.charCodeAt(1) === 0x21) {
    var nl = source.indexOf('\n');
    source = nl >= 0 ? source.slice(nl + 1) : '';
  }
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
    paths: [],
    require: null,
    _compile: null,
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
    var __M = globalThis.__cjs_builtins && globalThis.__cjs_builtins.get("module");
    if (__M && typeof __M._resolveFilename === "function") {
      return __M._resolveFilename(spec, { filename: resolved, id: resolved });
    }
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
  mod.require = moduleRequire;
  mod._compile = function(content, filename) {
    var __MC = globalThis.__cjs_builtins && globalThis.__cjs_builtins.get("module");
    var wrapped;
    if (__MC && typeof __MC.wrap === "function") {
      wrapped = __MC.wrap(content);
    } else {
      wrapped = "(function(exports,require,module,__filename,__dirname){\n" + content + "\n})";
    }
    const fn_ = (0, eval)(wrapped);
    const dir = filename.substring(0, filename.lastIndexOf("/"));
    fn_(mod.exports, moduleRequire, mod, filename, dir);
  };

  // Use Module.wrap to wrap source (allows frameworks like unio-container to inject scoped globals).
  // Parameter order matches Node.js: (exports, require, module, __filename, __dirname).
  var __ModuleClass = globalThis.__cjs_builtins && globalThis.__cjs_builtins.get("module");
  var wrapped;
  if (__ModuleClass && typeof __ModuleClass.wrap === "function") {
    wrapped = __ModuleClass.wrap(source);
  } else {
    wrapped =
      "(function(exports,require,module,__filename,__dirname){\n" +
      source +
      "\n})";
  }
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
    fn_(mod.exports, moduleRequire, mod, resolved, dirname);
  } catch (e) {
    Deno.core.ops.op_console_error("[cjs_loader] runtime error in " + resolved + ": " + e.message);
    // In snapshot mode, swallow CallSite-related errors from packages like
    // depd/http-errors. V8 snapshot creator produces broken CallSite objects
    // (undefined entries) which crash getFileName()/getLineNumber() etc.
    // These errors are non-fatal -- the module's exports are still usable.
    if (globalThis.__utoo_snapshot_mode && e instanceof TypeError &&
        (e.message.indexOf("getFileName") !== -1 ||
         e.message.indexOf("getLineNumber") !== -1 ||
         e.message.indexOf("getColumnNumber") !== -1 ||
         e.message.indexOf("CallSite") !== -1)) {
      Deno.core.ops.op_console_warn("[cjs_loader] suppressed CallSite error in snapshot mode: " + resolved);
    } else {
      throw e;
    }
  }
  globalThis.__cjs_current_file = prev;
  mod.loaded = true;

  return mod.exports;
}

__cjs_require.resolve = function (spec) {
  var __M = globalThis.__cjs_builtins && globalThis.__cjs_builtins.get("module");
  if (__M && typeof __M._resolveFilename === "function") {
    return __M._resolveFilename(spec, { filename: globalThis.__cjs_current_file, id: globalThis.__cjs_current_file });
  }
  return __cjsOps.op_cjs_resolve(spec, globalThis.__cjs_current_file);
};
__cjs_require.cache = __cjs_cache;

globalThis.require = __cjs_require;
