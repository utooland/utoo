// Minimal module module stub for utoo-runtime

class Module {
  constructor(id, parent) {
    this.id = id || "";
    this.path = "";
    this.exports = {};
    this.filename = null;
    this.loaded = false;
    this.children = [];
    this.paths = [];
    this.parent = parent || null;
  }

  require(id) {
    return globalThis.require(id);
  }
}

Module._cache = {};
Module._pathCache = {};
Module._extensions = {
  ".js": function() {},
  ".json": function() {},
  ".node": function() {},
  ".ts": function() {},
  ".cjs": function() {},
};
Module._nodeModulePaths = function(from) {
  const sep = '/';
  const parts = from.split(sep);
  const dirs = [];
  for (let i = parts.length; i > 0; i--) {
    if (parts[i - 1] === 'node_modules') continue;
    dirs.push(parts.slice(0, i).join(sep) + sep + 'node_modules');
  }
  return dirs;
};
Module._resolveFilename = function(request, parent) {
  // Use our CJS resolver for real resolution
  const ops = Deno.core.ops;
  let fromPath = '';
  if (parent && parent.filename) {
    fromPath = parent.filename;
  } else if (parent && parent.id) {
    fromPath = parent.id;
  }
  return ops.op_cjs_resolve(request, fromPath);
};
Module.builtinModules = [
  "assert", "async_hooks", "buffer", "child_process", "cluster",
  "console", "constants", "crypto", "dgram", "diagnostics_channel",
  "dns", "domain", "events", "fs", "http", "https",
  "module", "net", "os", "path", "perf_hooks", "querystring",
  "readline", "stream", "string_decoder", "timers", "tls", "tty",
  "url", "util", "v8", "worker_threads", "zlib",
];
Module.createRequire = function(filename) {
  let filepath = typeof filename === "string" ? filename :
    (filename && filename.pathname) ? filename.pathname : String(filename);
  // Convert file:// URLs to paths
  if (filepath.startsWith("file://")) {
    filepath = filepath.slice(7);
    // Handle Windows drive letters if needed (file:///C:/...)
  }
  function createdRequire(specifier) {
    const ops = Deno.core.ops;
    const resolved = ops.op_cjs_resolve(specifier, filepath);
    if (resolved.startsWith("node:")) {
      const builtins = globalThis.__cjs_builtins;
      if (builtins) {
        if (builtins.has(resolved)) return builtins.get(resolved);
        const name = resolved.slice(5);
        if (builtins.has(name)) return builtins.get(name);
      }
      throw new Error("Cannot find built-in module '" + resolved + "'");
    }
    if (globalThis.__cjs_cache && globalThis.__cjs_cache.has(resolved)) {
      return globalThis.__cjs_cache.get(resolved).exports;
    }
    // Set the current file context so global require resolves correctly
    const prevFile = globalThis.__cjs_current_file;
    globalThis.__cjs_current_file = filepath;
    try {
      return globalThis.require(specifier);
    } finally {
      globalThis.__cjs_current_file = prevFile;
    }
  }
  createdRequire.resolve = function(specifier) {
    const ops = Deno.core.ops;
    return ops.op_cjs_resolve(specifier, filepath);
  };
  createdRequire.cache = {};
  createdRequire.main = undefined;
  createdRequire.extensions = Module._extensions;
  return createdRequire;
};
Module.createRequireFromPath = Module.createRequire;
Module.isBuiltin = function(moduleName) {
  const name = moduleName.startsWith("node:") ? moduleName.slice(5) : moduleName;
  return Module.builtinModules.includes(name);
};
Module.wrap = function(script) {
  return "(function(exports,require,module,__filename,__dirname){" + script + "\n});";
};
Module._preloadModules = function(requests) {
  if (!Array.isArray(requests) || requests.length === 0) return;
  for (const request of requests) {
    try {
      globalThis.require(request);
    } catch (e) {
      Deno.core.ops.op_console_error("[module] _preloadModules failed for " + request + ": " + e.message);
    }
  }
};
Module._load = function(request, parent, isMain) {
  const ops = Deno.core.ops;
  let fromPath = '';
  if (parent && parent.filename) fromPath = parent.filename;
  else if (parent && parent.id) fromPath = parent.id;
  const resolved = ops.op_cjs_resolve(request, fromPath);
  if (resolved.startsWith("node:")) {
    const builtins = globalThis.__cjs_builtins;
    if (builtins) {
      if (builtins.has(resolved)) return builtins.get(resolved);
      const name = resolved.slice(5);
      if (builtins.has(name)) return builtins.get(name);
    }
    throw new Error("Cannot find built-in module '" + resolved + "'");
  }
  return globalThis.require(request);
};
Module._compile = function() {};
Module.runMain = function() {};

// Node.js compat: require('module') returns the Module class itself
Module.Module = Module;
Module.default = Module;

export default Module;
export const createRequire = Module.createRequire;
export const builtinModules = Module.builtinModules;
export const isBuiltin = Module.isBuiltin;
